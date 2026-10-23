//! Minimal, dependency-free editor for the INI dialect rclone uses for `rclone.conf`.
//!
//! S3Dive owns this file (it is never the user's `~/.config/rclone/rclone.conf`), and
//! every rclone invocation passes `--config <that path>` so the two never collide.
//! Comments and unrelated sections are preserved byte for byte.

use crate::model::Remote;

pub const SECTION_TYPE: &str = "type";
pub const SECTION_PROVIDER: &str = "provider";
pub const SECTION_ACCESS_KEY_ID: &str = "access_key_id";
pub const SECTION_SECRET_ACCESS_KEY: &str = "secret_access_key";
pub const SECTION_REGION: &str = "region";
pub const SECTION_ENDPOINT: &str = "endpoint";
pub const SECTION_FORCE_PATH_STYLE: &str = "force_path_style";

fn is_header(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('[') && trimmed.ends_with(']') {
        Some(trimmed[1..trimmed.len() - 1].trim())
    } else {
        None
    }
}

fn parse_pair(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
        return None;
    }
    let (key, value) = trimmed.split_once('=')?;
    let key = key.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    Some((key, value.trim().to_string()))
}

/// Section names in file order.
pub fn section_names(conf: &str) -> Vec<String> {
    conf.lines()
        .filter_map(is_header)
        .map(str::to_string)
        .collect()
}

/// Key/value pairs of one section, comments excluded.
pub fn get_section(conf: &str, name: &str) -> Option<Vec<(String, String)>> {
    let mut inside = false;
    let mut pairs = Vec::new();
    let mut found = false;

    for line in conf.lines() {
        if let Some(header) = is_header(line) {
            if inside {
                break;
            }
            inside = header == name;
            found |= inside;
            continue;
        }
        if inside {
            if let Some(pair) = parse_pair(line) {
                pairs.push(pair);
            }
        }
    }

    if found {
        Some(pairs)
    } else {
        None
    }
}

/// Replace a section's body, appending the section when it does not exist yet.
pub fn set_section(conf: &str, name: &str, pairs: &[(String, String)]) -> String {
    let mut lines: Vec<String> = conf.lines().map(str::to_string).collect();

    let mut start = None;
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        if let Some(header) = is_header(line) {
            if start.is_some() {
                end = index;
                break;
            }
            if header == name {
                start = Some(index);
            }
        }
    }

    let mut body: Vec<String> = pairs
        .iter()
        .map(|(key, value)| format!("{key} = {value}"))
        .collect();

    match start {
        Some(index) => {
            let mut next: Vec<String> = lines.drain(..=index).collect();
            lines.drain(..end - index - 1);
            next.append(&mut body);
            next.extend(lines);
            lines = next;
        }
        None => {
            while lines.last().is_some_and(|line| line.trim().is_empty()) {
                lines.pop();
            }
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(format!("[{name}]"));
            lines.append(&mut body);
        }
    }

    let mut result = lines.join("\n");
    result.push('\n');
    result
}

/// Drop a whole section, leaving every other line untouched.
pub fn remove_section(conf: &str, name: &str) -> String {
    let mut lines: Vec<String> = conf.lines().map(str::to_string).collect();
    let mut start = None;

    for (index, line) in lines.iter().enumerate() {
        if let Some(header) = is_header(line) {
            if header == name {
                start = Some(index);
                break;
            }
        }
    }

    let Some(start) = start else {
        return conf.to_string();
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| is_header(line).is_some())
        .map(|offset| start + 1 + offset)
        .unwrap_or(lines.len());

    // Also swallow the blank line rclone-style sections are separated by.
    let mut end = end;
    if end < lines.len() && lines[end].trim().is_empty() {
        end += 1;
    }
    lines.drain(start..end);

    if lines.is_empty() {
        return String::new();
    }
    let mut result = lines.join("\n");
    result.push('\n');
    result
}

/// The config section body for an S3 remote.
///
/// `secret` is the plaintext secret access key. rclone's s3 backend reads
/// `secret_access_key` verbatim and never reveals an obscured value, so the value is
/// written as given; callers keep it out of logs and process arguments.
pub fn remote_pairs(remote: &Remote, secret: &str) -> Vec<(String, String)> {
    let mut pairs = vec![
        (SECTION_TYPE.to_string(), "s3".to_string()),
        (
            SECTION_PROVIDER.to_string(),
            remote.provider.rclone_value().to_string(),
        ),
        (
            SECTION_ACCESS_KEY_ID.to_string(),
            remote.access_key_id.trim().to_string(),
        ),
        (
            SECTION_SECRET_ACCESS_KEY.to_string(),
            secret.trim().to_string(),
        ),
        (SECTION_REGION.to_string(), remote.region.trim().to_string()),
    ];

    if let Some(endpoint) = remote
        .endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        pairs.push((SECTION_ENDPOINT.to_string(), endpoint.to_string()));
    }
    if remote.force_path_style {
        pairs.push((SECTION_FORCE_PATH_STYLE.to_string(), "true".to_string()));
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::S3Provider;

    // A realistic Backblaze B2 application key: 31 characters, base64-ish, and it
    // contains a slash, the shape that broke when it was obscured on the way to disk.
    const SECRET: &str = "0055aa7b8c9d0e1f2a3b4c5d6S/7890";

    fn remote() -> Remote {
        let mut remote = Remote::new("media", "AKIAEXAMPLE");
        remote.region = "eu-west-1".into();
        remote
    }

    #[test]
    fn new_sections_are_appended_with_a_blank_separator() {
        let conf = set_section("", "media", &remote_pairs(&remote(), SECRET));
        assert!(conf.starts_with("[media]\n"), "{conf}");
        assert!(conf.contains("type = s3\n"), "{conf}");
        assert!(conf.contains("provider = AWS\n"), "{conf}");
        assert!(conf.contains("access_key_id = AKIAEXAMPLE\n"), "{conf}");
        assert!(
            conf.contains(&format!("secret_access_key = {SECRET}\n")),
            "{conf}"
        );
        assert!(conf.contains("region = eu-west-1\n"), "{conf}");
        assert!(conf.ends_with('\n'), "{conf}");
    }

    #[test]
    fn overwriting_a_section_keeps_other_sections_and_order() {
        let first = set_section("", "media", &remote_pairs(&remote(), SECRET));
        let both = set_section(
            &first,
            "backup",
            &remote_pairs(&Remote::new("backup", "AKIABACKUP"), "other"),
        );

        assert_eq!(section_names(&both), vec!["media", "backup"]);

        let mut changed = remote();
        changed.access_key_id = "AKIANEW".into();
        changed.region = "us-west-2".into();
        let updated = set_section(&both, "media", &remote_pairs(&changed, "newsecret"));

        assert_eq!(section_names(&updated), vec!["media", "backup"]);
        assert!(updated.contains("access_key_id = AKIANEW"), "{updated}");
        assert!(updated.contains("region = us-west-2"), "{updated}");
        assert!(!updated.contains("AKIAEXAMPLE"), "{updated}");
        assert!(updated.contains("[backup]"), "{updated}");
        assert_eq!(
            get_section(&updated, "media"),
            Some(remote_pairs(&changed, "newsecret"))
        );
    }

    // rclone's s3 backend signs with whatever `secret_access_key` holds, so an obscured
    // value fails every request with SignatureDoesNotMatch. The secret must reach the
    // file byte for byte.
    #[test]
    fn secret_is_written_verbatim() {
        let conf = set_section("", "media", &remote_pairs(&remote(), SECRET));
        let pairs = get_section(&conf, "media").expect("section");
        let secret = pairs
            .iter()
            .find(|(key, _)| key == SECTION_SECRET_ACCESS_KEY)
            .map(|(_, value)| value.clone())
            .expect("secret key present");
        assert_eq!(secret, SECRET);
    }

    #[test]
    fn endpoint_and_path_style_are_optional() {
        let mut custom = remote();
        custom.provider = S3Provider::Other;
        custom.endpoint = Some("https://minio.local:9000".into());
        custom.force_path_style = true;

        let pairs = remote_pairs(&custom, SECRET);
        let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
        assert!(keys.contains(&SECTION_ENDPOINT));
        assert!(keys.contains(&SECTION_FORCE_PATH_STYLE));
        assert_eq!(
            pairs
                .iter()
                .find(|(key, _)| key == SECTION_PROVIDER)
                .map(|(_, value)| value.as_str()),
            Some("Other")
        );

        let mut plain = remote();
        plain.endpoint = Some("   ".into());
        let keys: Vec<String> = remote_pairs(&plain, SECRET)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert!(!keys.contains(&SECTION_ENDPOINT.to_string()));
    }

    #[test]
    fn removing_a_section_leaves_the_rest_intact() {
        let first = set_section("", "media", &remote_pairs(&remote(), SECRET));
        let both = set_section(
            &first,
            "backup",
            &remote_pairs(&Remote::new("backup", "AKIA"), "x"),
        );

        let removed = remove_section(&both, "media");
        assert_eq!(section_names(&removed), vec!["backup"]);
        assert!(!removed.contains("AKIAEXAMPLE"), "{removed}");
        assert!(get_section(&removed, "media").is_none());

        let untouched = remove_section(&removed, "absent");
        assert_eq!(untouched, removed);

        assert_eq!(
            remove_section(&both, "media"),
            set_section(
                "",
                "backup",
                &remote_pairs(&Remote::new("backup", "AKIA"), "x")
            )
        );
    }

    #[test]
    fn comments_and_unknown_keys_are_preserved_inside_other_sections() {
        let original = "# my notes\n[other]\ntype = local\n# keep me\n";
        let conf = set_section(original, "media", &remote_pairs(&remote(), SECRET));
        assert!(conf.contains("# my notes"), "{conf}");
        assert!(conf.contains("# keep me"), "{conf}");
        assert_eq!(section_names(&conf), vec!["other", "media"]);
    }

    #[test]
    fn section_lookup_is_exact() {
        let conf = "[media]\nregion = x\n[media2]\nregion = y\n";
        assert_eq!(
            get_section(conf, "media"),
            Some(vec![("region".to_string(), "x".to_string())])
        );
        assert_eq!(
            get_section(conf, "media2"),
            Some(vec![("region".to_string(), "y".to_string())])
        );
        assert_eq!(get_section(conf, "missing"), None);
    }
}
