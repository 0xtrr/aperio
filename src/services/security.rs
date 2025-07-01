use crate::error::{AppError, AppResult};
use url::Url;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub struct SecurityValidator {
    allowed_domains: Vec<String>,
    max_url_length: usize,
    max_file_size_bytes: u64,
}

impl SecurityValidator {
    pub fn new(allowed_domains: Vec<String>, max_file_size_mb: u32, max_url_length: u32) -> Self {
        Self {
            allowed_domains,
            max_url_length: max_url_length as usize,
            max_file_size_bytes: (max_file_size_mb as u64) * 1024 * 1024, // Convert MB to bytes
        }
    }

    /// Comprehensive URL validation with security checks
    pub fn validate_url(&self, url_str: &str) -> AppResult<Url> {
        // Check URL length to prevent DoS
        if url_str.len() > self.max_url_length {
            return Err(AppError::Download(format!(
                "URL too long: {} characters (max: {})",
                url_str.len(),
                self.max_url_length
            )));
        }

        // Basic URL parsing
        let url = Url::parse(url_str).map_err(|e| {
            AppError::Download(format!("Invalid URL format: {e}"))
        })?;

        // Ensure HTTPS only (security requirement)
        if url.scheme() != "https" {
            return Err(AppError::Download(
                "Only HTTPS URLs are allowed for security reasons".to_string()
            ));
        }

        // Validate host exists
        let host = url.host_str().ok_or_else(|| {
            AppError::Download("URL must have a valid host".to_string())
        })?;

        // Prevent access to internal/private networks
        self.validate_host_security(host)?;

        // Validate domain is in allowed list
        if !self.is_domain_allowed(host) {
            return Err(AppError::Download(format!(
                "Domain '{}' is not in the allowed domains list: {}",
                host,
                self.allowed_domains.join(", ")
            )));
        }

        // Check for suspicious URL patterns
        self.validate_url_patterns(&url)?;

        Ok(url)
    }

    /// Validate input data for security issues
    pub fn validate_input(&self, input: &str, field_name: &str, max_length: usize) -> AppResult<()> {
        // Check length
        if input.len() > max_length {
            return Err(AppError::BadRequest(format!(
                "{} too long: {} characters (max: {})",
                field_name, input.len(), max_length
            )));
        }

        // Check for null bytes (security risk)
        if input.contains('\0') {
            return Err(AppError::BadRequest(format!(
                "{field_name} contains null bytes"
            )));
        }

        // Check for control characters (except newlines and tabs)
        if input.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
            return Err(AppError::BadRequest(format!(
                "{field_name} contains invalid control characters"
            )));
        }

        // Additional validation for job IDs to prevent path traversal
        if field_name == "job_id" {
            self.validate_job_id(input)?;
        }

        Ok(())
    }

    /// Validate job ID to prevent path traversal attacks
    pub fn validate_job_id(&self, job_id: &str) -> AppResult<()> {
        // Check for path traversal attempts
        if job_id.contains("..") || job_id.contains("/") || job_id.contains("\\") {
            return Err(AppError::BadRequest(
                "Job ID contains invalid path characters".to_string()
            ));
        }

        // Ensure job ID only contains safe characters (alphanumeric, hyphens, underscores)
        if !job_id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(AppError::BadRequest(
                "Job ID contains invalid characters".to_string()
            ));
        }

        // Ensure job ID is not empty and has reasonable length
        if job_id.is_empty() || job_id.len() > 100 {
            return Err(AppError::BadRequest(
                "Job ID must be between 1 and 100 characters".to_string()
            ));
        }

        Ok(())
    }

    /// Safely construct file path for job, preventing directory traversal
    pub fn safe_job_file_path(&self, base_dir: &std::path::Path, job_id: &str, filename: &str) -> AppResult<std::path::PathBuf> {
        // Validate inputs
        self.validate_job_id(job_id)?;
        
        // Validate filename (no path separators, no hidden files)
        if filename.contains("/") || filename.contains("\\") || filename.contains("..") || filename.starts_with('.') {
            return Err(AppError::BadRequest(
                "Invalid filename".to_string()
            ));
        }

        // Construct safe path
        let safe_path = base_dir.join(format!("{job_id}_{filename}"));
        
        // Ensure the resulting path is still within the base directory
        if let Ok(canonical_base) = base_dir.canonicalize() {
            if let Ok(canonical_path) = safe_path.canonicalize() {
                if !canonical_path.starts_with(canonical_base) {
                    return Err(AppError::BadRequest(
                        "Path traversal attempt detected".to_string()
                    ));
                }
            }
        }

        Ok(safe_path)
    }

    pub fn get_max_file_size(&self) -> u64 {
        self.max_file_size_bytes
    }

    // Private helper methods

    fn validate_host_security(&self, host: &str) -> AppResult<()> {
        // Try to parse as IP address first
        if let Ok(ip) = host.parse::<IpAddr>() {
            return self.validate_ip_address(&ip);
        }

        // For domain names, check for suspicious patterns
        if host.is_empty() {
            return Err(AppError::Download("Empty host not allowed".to_string()));
        }

        // Prevent localhost variants
        let host_lower = host.to_lowercase();
        if host_lower == "localhost" 
            || host_lower.ends_with(".localhost") 
            || host_lower.ends_with(".local") {
            return Err(AppError::Download(
                "Access to localhost/local domains is not allowed".to_string()
            ));
        }

        // Prevent internal domain access
        if host_lower.ends_with(".internal") 
            || host_lower.ends_with(".intranet") 
            || host_lower.contains("internal.") {
            return Err(AppError::Download(
                "Access to internal domains is not allowed".to_string()
            ));
        }

        Ok(())
    }

    fn validate_ip_address(&self, ip: &IpAddr) -> AppResult<()> {
        match ip {
            IpAddr::V4(ipv4) => self.validate_ipv4_address(ipv4),
            IpAddr::V6(ipv6) => self.validate_ipv6_address(ipv6),
        }
    }

    fn validate_ipv4_address(&self, ip: &Ipv4Addr) -> AppResult<()> {
        // Block private/internal IP ranges
        if ip.is_private() {
            return Err(AppError::Download(
                "Access to private IP addresses is not allowed".to_string()
            ));
        }

        if ip.is_loopback() {
            return Err(AppError::Download(
                "Access to loopback addresses is not allowed".to_string()
            ));
        }

        if ip.is_link_local() {
            return Err(AppError::Download(
                "Access to link-local addresses is not allowed".to_string()
            ));
        }

        if ip.is_multicast() {
            return Err(AppError::Download(
                "Access to multicast addresses is not allowed".to_string()
            ));
        }

        // Block additional internal ranges
        let octets = ip.octets();
        
        // Block CGN (100.64.0.0/10)
        if octets[0] == 100 && (octets[1] & 0xC0) == 64 {
            return Err(AppError::Download(
                "Access to CGN addresses is not allowed".to_string()
            ));
        }

        Ok(())
    }

    fn validate_ipv6_address(&self, ip: &Ipv6Addr) -> AppResult<()> {
        // Block loopback addresses
        if ip.is_loopback() {
            return Err(AppError::Download(
                "Access to loopback addresses is not allowed".to_string()
            ));
        }

        // Block unspecified addresses (::)
        if ip.is_unspecified() {
            return Err(AppError::Download(
                "Access to unspecified addresses is not allowed".to_string()
            ));
        }

        // Block multicast addresses
        if ip.is_multicast() {
            return Err(AppError::Download(
                "Access to multicast addresses is not allowed".to_string()
            ));
        }

        // Block link-local addresses (fe80::/10)
        if (ip.segments()[0] & 0xffc0) == 0xfe80 {
            return Err(AppError::Download(
                "Access to link-local addresses is not allowed".to_string()
            ));
        }

        // Block unique local addresses (fc00::/7) - private IPv6 ranges
        if (ip.segments()[0] & 0xfe00) == 0xfc00 {
            return Err(AppError::Download(
                "Access to unique local addresses is not allowed".to_string()
            ));
        }

        Ok(())
    }

    fn is_domain_allowed(&self, host: &str) -> bool {
        self.allowed_domains.iter().any(|domain| {
            // Exact match or subdomain match
            host == domain || host.ends_with(&format!(".{domain}"))
        })
    }

    fn validate_url_patterns(&self, url: &Url) -> AppResult<()> {
        let url_string = url.as_str();

        // Check for suspicious URL patterns
        if url_string.contains("@") && !url_string.contains("youtube.com") {
            return Err(AppError::Download(
                "URLs with @ symbols are not allowed (potential redirect attack)".to_string()
            ));
        }

        // Check for encoded characters that might bypass validation
        if url_string.contains("%2F") || url_string.contains("%5C") {
            return Err(AppError::Download(
                "URLs with encoded slashes are not allowed".to_string()
            ));
        }

        // Check for double slashes in path (except after protocol)
        if let Some(path) = url.path_segments() {
            for segment in path {
                if segment.contains("..") {
                    return Err(AppError::Download(
                        "URLs with path traversal patterns are not allowed".to_string()
                    ));
                }
            }
        }

        Ok(())
    }
}
