#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchPart {
    pub index: usize,
    pub commit: Option<String>,
    pub subject: String,
    pub filename: String,
    pub content: String,
}

pub fn split_patch_by_commit(input: &str) -> Vec<PatchPart> {
    if input.trim().is_empty() {
        return Vec::new();
    }

    let mut starts = commit_starts(input);
    if starts.is_empty() {
        starts.push(0);
    }

    starts
        .iter()
        .enumerate()
        .map(|(position, start)| {
            let end = starts
                .get(position + 1)
                .copied()
                .unwrap_or_else(|| input.len());
            let content = input[*start..end].to_string();
            let index = position + 1;
            let commit = extract_commit(&content);
            let subject = extract_subject(&content).unwrap_or_else(|| format!("patch-{index}"));
            let filename = patch_filename(index, &subject);

            PatchPart {
                index,
                commit,
                subject,
                filename,
                content,
            }
        })
        .collect()
}

pub fn patch_filename(index: usize, subject: &str) -> String {
    let slug = slugify(subject);
    format!("{index:04}-{}.patch", if slug.is_empty() { "patch" } else { &slug })
}

fn commit_starts(input: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut offset = 0;

    for line in input.split_inclusive('\n') {
        if extract_commit_from_marker(line).is_some() {
            starts.push(offset);
        }
        offset += line.len();
    }

    if offset < input.len() {
        let line = &input[offset..];
        if extract_commit_from_marker(line).is_some() {
            starts.push(offset);
        }
    }

    starts
}

fn extract_commit(content: &str) -> Option<String> {
    content.lines().next().and_then(extract_commit_from_marker)
}

fn extract_commit_from_marker(line: &str) -> Option<String> {
    let line = line.trim_end_matches(['\r', '\n']);
    let rest = line.strip_prefix("From ")?;
    let hash = rest.get(..40)?;
    let separator = rest.as_bytes().get(40)?;

    if *separator == b' ' && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(hash.to_string())
    } else {
        None
    }
}

fn extract_subject(content: &str) -> Option<String> {
    let mut subject = None::<String>;
    let mut reading_subject = false;

    for (line_index, raw_line) in content.lines().enumerate() {
        if line_index == 0 && extract_commit_from_marker(raw_line).is_some() {
            continue;
        }

        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }

        if reading_subject && (line.starts_with(' ') || line.starts_with('\t')) {
            if let Some(value) = subject.as_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }

        reading_subject = false;
        if let Some(value) = line.strip_prefix("Subject:") {
            subject = Some(clean_subject(value));
            reading_subject = true;
        }
    }

    subject.filter(|value| !value.is_empty())
}

fn clean_subject(subject: &str) -> String {
    let mut value = subject.trim();

    loop {
        let Some(rest) = value.strip_prefix('[') else {
            break;
        };
        let Some(end) = rest.find(']') else {
            break;
        };
        let tag = &rest[..end];
        if !tag.to_ascii_lowercase().starts_with("patch") {
            break;
        }
        value = rest[end + 1..].trim_start();
    }

    value.trim().to_string()
}

fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut pending_separator = false;

    for character in input.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            pending_separator = false;
        } else {
            pending_separator = true;
        }

        if slug.len() >= 60 {
            break;
        }
    }

    slug.trim_matches('-').to_string()
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::{patch_filename, split_patch_by_commit};

    const FIRST_SHA: &str = "1111111111111111111111111111111111111111";
    const SECOND_SHA: &str = "2222222222222222222222222222222222222222";

    #[test]
    fn splits_github_pr_patch_by_from_markers() {
        let input = format!(
            "\
From {FIRST_SHA} Mon Sep 17 00:00:00 2001
From: A <a@example.com>
Date: Sun, 9 Aug 2026 12:00:00 +0800
Subject: [PATCH 1/2] Add parser

diff --git a/a.txt b/a.txt
new file mode 100644
--- /dev/null
+++ b/a.txt
@@ -0,0 +1 @@
+a
From {SECOND_SHA} Mon Sep 17 00:00:00 2001
From: A <a@example.com>
Date: Sun, 9 Aug 2026 12:01:00 +0800
Subject: [PATCH 2/2] Wire CLI

diff --git a/b.txt b/b.txt
new file mode 100644
--- /dev/null
+++ b/b.txt
@@ -0,0 +1 @@
+b
"
        );

        let parts = split_patch_by_commit(&input);

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].commit.as_deref(), Some(FIRST_SHA));
        assert_eq!(parts[0].subject, "Add parser");
        assert_eq!(parts[0].filename, "0001-add-parser.patch");
        assert!(parts[0].content.starts_with(&format!("From {FIRST_SHA}")));
        assert!(!parts[0].content.contains(SECOND_SHA));
        assert_eq!(parts[1].commit.as_deref(), Some(SECOND_SHA));
        assert_eq!(parts[1].subject, "Wire CLI");
        assert_eq!(parts[1].filename, "0002-wire-cli.patch");
    }

    #[test]
    fn keeps_single_patch_without_commit_marker() {
        let input = "\
From: A <a@example.com>
Subject: Manual patch

diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-old
+new
";

        let parts = split_patch_by_commit(input);

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].commit, None);
        assert_eq!(parts[0].subject, "Manual patch");
        assert_eq!(parts[0].content, input);
    }

    #[test]
    fn joins_folded_subject_headers() {
        let input = format!(
            "\
From {FIRST_SHA} Mon Sep 17 00:00:00 2001
Subject: [PATCH v2 1/1] Add a long
 subject header

diff --git a/a.txt b/a.txt
"
        );

        let parts = split_patch_by_commit(&input);

        assert_eq!(parts[0].subject, "Add a long subject header");
        assert_eq!(parts[0].filename, "0001-add-a-long-subject-header.patch");
    }

    #[test]
    fn creates_portable_patch_file_names() {
        assert_eq!(
            patch_filename(7, "[not cleaned here] Fix: path/to file?!"),
            "0007-not-cleaned-here-fix-path-to-file.patch"
        );
    }
}
