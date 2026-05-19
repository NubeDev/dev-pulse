//! `Co-authored-by:` trailer parsing for commit messages.
//!
//! Per SCOPE §6, co-authored commits must credit every co-author —
//! not just the primary author the API surfaces. GitHub itself
//! parses these out of the commit message footer rather than
//! exposing them as structured fields, so we do the same.
//!
//! Trailer wire form (one per line, in the commit message footer):
//!
//! ```text
//! Co-authored-by: Octocat <octocat@github.com>
//! Co-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>
//! ```
//!
//! We only extract `(name, email)`. Linking to a GitHub user
//! happens later (the worker tries `email -> users.email` first,
//! then falls back to the GitHub `noreply` convention of
//! `<id>+<login>@users.noreply.github.com`).

/// One parsed `Co-authored-by:` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoAuthor {
    /// Display name as it appeared in the trailer.
    pub name: String,
    /// Email as it appeared in the trailer.
    pub email: String,
}

impl CoAuthor {
    /// GitHub's `<id>+<login>@users.noreply.github.com` convention.
    /// Returns `Some(login)` when the email matches the pattern,
    /// `None` otherwise. The worker uses this to attribute a
    /// co-author when the email-to-user lookup fails.
    pub fn noreply_login(&self) -> Option<&str> {
        let local = self.email.split('@').next()?;
        let (_, login) = local.split_once('+')?;
        // The `+login` form requires a non-empty login segment.
        if login.is_empty() {
            None
        } else {
            Some(login)
        }
    }
}

/// Pull every `Co-authored-by:` trailer out of `message`. Order
/// is preserved (matches how GitHub renders the commit page).
/// Match is case-insensitive on the key per the Git convention.
pub fn parse_coauthors(message: &str) -> Vec<CoAuthor> {
    let mut out = Vec::new();
    for line in message.lines() {
        // Trailers live in the footer — but Git accepts them
        // anywhere in the message and so do we. Trim leading WS
        // because some clients indent the footer.
        let line = line.trim();
        let Some(rest) = strip_prefix_ci(line, "co-authored-by:") else {
            continue;
        };
        let rest = rest.trim();
        // `Name <email>` — the angle brackets are mandatory per
        // the Git trailer spec. Drop anything that doesn't parse;
        // we'd rather lose a malformed trailer than misattribute.
        let Some(lt) = rest.rfind('<') else { continue };
        let Some(gt) = rest.rfind('>') else { continue };
        if gt <= lt {
            continue;
        }
        let name = rest[..lt].trim().trim_end_matches(',').trim().to_string();
        let email = rest[lt + 1..gt].trim().to_string();
        if name.is_empty() || email.is_empty() {
            continue;
        }
        out.push(CoAuthor { name, email });
    }
    out
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() {
        return None;
    }
    let (head, tail) = s.split_at(prefix.len());
    if head.eq_ignore_ascii_case(prefix) {
        Some(tail)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_single_coauthor() {
        let msg = "feat: thing\n\nCo-authored-by: Octocat <octocat@github.com>\n";
        let got = parse_coauthors(msg);
        assert_eq!(
            got,
            vec![CoAuthor {
                name: "Octocat".into(),
                email: "octocat@github.com".into(),
            }]
        );
    }

    #[test]
    fn extracts_multiple_in_order() {
        let msg = "\
fix: ship it

Co-authored-by: A <a@example.com>
Co-authored-by: B <b@example.com>
";
        let got = parse_coauthors(msg);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "A");
        assert_eq!(got[1].name, "B");
    }

    #[test]
    fn is_case_insensitive_on_the_key() {
        let msg = "CO-AUTHORED-BY: Octo <o@example.com>";
        assert_eq!(parse_coauthors(msg).len(), 1);
    }

    #[test]
    fn ignores_malformed_lines() {
        let msg = "\
Co-authored-by: missing-angles@example.com
Co-authored-by:
Co-authored-by: <empty-name@example.com>
Co-authored-by: empty-email <>
Co-authored-by: Good <good@example.com>
";
        let got = parse_coauthors(msg);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "Good");
    }

    #[test]
    fn noreply_login_extracted() {
        let a = CoAuthor {
            name: "Octocat".into(),
            email: "12345+octocat@users.noreply.github.com".into(),
        };
        assert_eq!(a.noreply_login(), Some("octocat"));

        let b = CoAuthor {
            name: "x".into(),
            email: "plain@example.com".into(),
        };
        assert_eq!(b.noreply_login(), None);
    }

    #[test]
    fn bot_trailer_is_kept_verbatim() {
        // Bot detection is the report layer's job — we still pull
        // the trailer out so the bot lands in event_actors.
        let msg = "chore: bump\n\nCo-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>";
        let got = parse_coauthors(msg);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "dependabot[bot]");
    }
}
