use chrono::NaiveDate;
use thiserror::Error;

/// A single day's worklog entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DayEntry {
    pub date: NaiveDate,
    pub items: Vec<String>,
}

/// A week of worklog entries.
#[derive(Debug, Clone, PartialEq)]
pub struct WeekEntry {
    pub week_number: u32,
    /// ISO year for this week (handles year boundaries correctly).
    /// `None` for entries parsed from markdown where the year is inferred from dates.
    pub iso_year: Option<i32>,
    pub days: Vec<DayEntry>,
}

/// Structured representation of the entire worklog.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkLog {
    pub weeks: Vec<WeekEntry>,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid week header: {0}")]
    InvalidWeekHeader(String),
}

/// Extract the week number from a line like `## Week N` or `## **Week N**`.
fn parse_week_header(line: &str) -> Option<Result<u32, ParseError>> {
    let after_hashes = line.strip_prefix("## ")?;
    // Strip optional bold markers around "Week N"
    let inner = after_hashes
        .strip_prefix("**")
        .and_then(|s| s.strip_suffix("**"))
        .unwrap_or(after_hashes);
    let number_str = inner.strip_prefix("Week ")?;
    let n = number_str
        .trim()
        .parse::<u32>()
        .map_err(|_| ParseError::InvalidWeekHeader(line.to_string()));
    Some(n)
}

/// Attempt to parse a date line like `Mar 9, 2026` or `Dec 10, 2025`.
/// Returns `None` for lines that are not dates (including partial matches),
/// so the caller can skip them without error.
fn parse_date_line(line: &str) -> Option<NaiveDate> {
    // Lines with leading `*` are items, not dates.
    if line.starts_with('*') {
        return None;
    }
    // Quick heuristic: date lines contain a comma followed by a 4-digit year.
    if !line.contains(',') {
        return None;
    }
    NaiveDate::parse_from_str(line.trim(), "%b %-d, %Y").ok()
}

/// Trim trailing continuation markers used in the archive (space+backslash or
/// two trailing spaces that act as Markdown line-breaks) without altering
/// intentional content.
fn clean_item(text: &str) -> String {
    let s = text.trim_end_matches('\\').trim_end();
    s.to_string()
}

/// Parse a worklog.md string into structured data.
///
/// The parser is intentionally lenient: it skips any preamble before the first
/// week header and ignores blank lines between sections.
pub fn parse_worklog(input: &str) -> Result<WorkLog, ParseError> {
    let mut weeks: Vec<WeekEntry> = Vec::new();
    let mut current_week: Option<WeekEntry> = None;
    let mut current_day: Option<DayEntry> = None;

    for raw_line in input.lines() {
        // Strip the trailing newline artifacts that may survive `lines()` on
        // Windows-style line endings; also normalise trailing whitespace so
        // the rest of the logic sees clean content.
        let line = raw_line.trim_end();

        if line.is_empty() {
            continue;
        }

        // --- Week header ---
        if line.starts_with("## ")
            && let Some(result) = parse_week_header(line)
        {
            let week_number = result?;

            // Commit any open day into the current week before starting a
            // new week.
            if let Some(day) = current_day.take()
                && let Some(ref mut week) = current_week
            {
                week.days.push(day);
            }
            if let Some(week) = current_week.take() {
                weeks.push(week);
            }
            current_week = Some(WeekEntry {
                week_number,
                iso_year: None,
                days: Vec::new(),
            });
            continue;
        }

        // --- Bullet item ---
        if let Some(rest) = line.strip_prefix("* ") {
            let text = clean_item(rest);
            if let Some(ref mut day) = current_day {
                day.items.push(text);
            }
            // Items outside a day context are silently ignored; this keeps the
            // parser robust against the preamble bullets in the archive.
            continue;
        }

        // --- Date line ---
        if let Some(date) = parse_date_line(line) {
            // Commit any previously open day.
            if let Some(day) = current_day.take()
                && let Some(ref mut week) = current_week
            {
                week.days.push(day);
            }
            current_day = Some(DayEntry {
                date,
                items: Vec::new(),
            });
        }
        // Lines that match none of the above (e.g. preamble prose) are skipped.
    }

    // Flush remaining open day and week.
    if let Some(day) = current_day.take()
        && let Some(ref mut week) = current_week
    {
        week.days.push(day);
    }
    if let Some(week) = current_week.take() {
        weeks.push(week);
    }

    Ok(WorkLog { weeks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    // --- Error / edge cases ---

    #[test]
    fn test_parse_empty_input() {
        let result = parse_worklog("").unwrap();
        assert_eq!(result.weeks, vec![], "empty input should yield no weeks");
    }

    #[test]
    fn test_parse_preamble_only() {
        let input = "Some preamble text\nwith multiple lines\nbut no week headers.";
        let result = parse_worklog(input).unwrap();
        assert_eq!(
            result.weeks,
            vec![],
            "preamble with no weeks should yield empty"
        );
    }

    #[test]
    fn test_parse_invalid_week_number_returns_error() {
        let input = "## Week abc\n";
        let result = parse_worklog(input);
        assert!(
            result.is_err(),
            "non-numeric week number should be an error"
        );
    }

    // --- Trivial cases ---

    #[test]
    fn test_parse_single_week_no_days() {
        let input = "## Week 1\n";
        let result = parse_worklog(input).unwrap();
        assert_eq!(
            result.weeks,
            vec![WeekEntry {
                week_number: 1,
                iso_year: None,
                days: vec![],
            }],
            "single week with no days"
        );
    }

    #[test]
    fn test_parse_single_week_one_day_one_item() {
        let input = "## Week 1\n\nJan 5, 2026\n\n* Did a thing\n";
        let result = parse_worklog(input).unwrap();
        assert_eq!(
            result.weeks,
            vec![WeekEntry {
                week_number: 1,
                iso_year: None,
                days: vec![DayEntry {
                    date: date(2026, 1, 5),
                    items: vec!["Did a thing".to_string()],
                }],
            }],
            "single week, one day, one item"
        );
    }

    #[test]
    fn test_parse_single_week_one_day_multiple_items() {
        let input = "## Week 5\n\nFeb 2, 2026\n\n* Item one\n* Item two\n* Item three\n";
        let result = parse_worklog(input).unwrap();
        assert_eq!(
            result.weeks[0].days[0].items,
            vec!["Item one", "Item two", "Item three"],
            "multiple items parsed in order"
        );
    }

    #[test]
    fn test_parse_single_week_multiple_days() {
        let input =
            "## Week 10\n\nMar 2, 2026\n\n* Morning standup\n\nMar 3, 2026\n\n* Code review\n";
        let result = parse_worklog(input).unwrap();
        let week = &result.weeks[0];
        assert_eq!(week.week_number, 10, "week number");
        assert_eq!(week.days.len(), 2, "two days in the week");
        assert_eq!(week.days[0].date, date(2026, 3, 2), "first day date");
        assert_eq!(week.days[1].date, date(2026, 3, 3), "second day date");
        assert_eq!(week.days[1].items, vec!["Code review"], "second day items");
    }

    // --- Multiple weeks ---

    #[test]
    fn test_parse_multiple_weeks() {
        let input =
            "## Week 1\n\nJan 5, 2026\n\n* Item A\n\n## Week 2\n\nJan 12, 2026\n\n* Item B\n";
        let result = parse_worklog(input).unwrap();
        assert_eq!(result.weeks.len(), 2, "two weeks parsed");
        assert_eq!(result.weeks[0].week_number, 1);
        assert_eq!(result.weeks[1].week_number, 2);
        assert_eq!(result.weeks[0].days[0].items, vec!["Item A"]);
        assert_eq!(result.weeks[1].days[0].items, vec!["Item B"]);
    }

    // --- Bold week headers (archive format) ---

    #[test]
    fn test_parse_bold_week_header() {
        let input = "## **Week 50**\n\nDec 9, 2025\n\n* Sprint planning\n";
        let result = parse_worklog(input).unwrap();
        assert_eq!(
            result.weeks[0].week_number, 50,
            "bold week header should parse correctly"
        );
        assert_eq!(result.weeks[0].days[0].date, date(2025, 12, 9));
    }

    // --- Trailing whitespace and backslash continuation (archive format) ---

    #[test]
    fn test_parse_items_with_trailing_backslash() {
        // The archive uses trailing `\` as Markdown line-break continuation.
        let input = "## Week 50\n\nDec 9, 2025\n\n* Made QA list  \\\n* Sprint meeting\n";
        let result = parse_worklog(input).unwrap();
        let items = &result.weeks[0].days[0].items;
        assert_eq!(
            items[0], "Made QA list",
            "trailing backslash should be stripped"
        );
        assert_eq!(items[1], "Sprint meeting");
    }

    #[test]
    fn test_parse_items_with_trailing_spaces() {
        // Markdown two-trailing-spaces line break — treat as whitespace only.
        let input = "## Week 11\n\nMar 9, 2026\n\n* Attended standup  \n";
        let result = parse_worklog(input).unwrap();
        assert_eq!(
            result.weeks[0].days[0].items[0], "Attended standup",
            "trailing spaces should be stripped from items"
        );
    }

    // --- Preamble before first week (archive format) ---

    #[test]
    fn test_parse_with_preamble_before_first_week() {
        let input = concat!(
            "Some preamble text here.\n",
            "\n",
            "* A preamble bullet\n",
            "\n",
            "## Week 50\n",
            "\n",
            "Dec 9, 2025\n",
            "\n",
            "* Real item\n",
        );
        let result = parse_worklog(input).unwrap();
        assert_eq!(
            result.weeks.len(),
            1,
            "preamble should not create extra weeks"
        );
        assert_eq!(result.weeks[0].week_number, 50);
        assert_eq!(
            result.weeks[0].days[0].items,
            vec!["Real item"],
            "only items inside a week/day should be captured"
        );
    }

    // --- Prose lines with commas should not cause errors ---

    #[test]
    fn test_parse_prose_with_comma_does_not_error() {
        let input = concat!(
            "## Week 50\n",
            "\n",
            "Dec 9, 2025\n",
            "\n",
            "* Made QA list\n",
            "Note: see Mar 5, above for details\n",
            "* Sprint meeting\n",
        );
        let result = parse_worklog(input).unwrap();
        assert_eq!(
            result.weeks[0].days[0].items,
            vec!["Made QA list", "Sprint meeting"],
            "prose lines with commas should be skipped, not cause errors"
        );
    }

    // --- Real-world data ---

    #[test]
    fn test_parse_real_worklog_snippet() {
        // Matches the actual current worklog.md content.
        let input = "## Week 11\n\nMar 9, 2026\n\n* Attended standup\n";
        let result = parse_worklog(input).unwrap();
        assert_eq!(result.weeks.len(), 1, "one week");
        assert_eq!(result.weeks[0].week_number, 11);
        assert_eq!(result.weeks[0].days.len(), 1, "one day");
        assert_eq!(result.weeks[0].days[0].date, date(2026, 3, 9));
        assert_eq!(
            result.weeks[0].days[0].items,
            vec!["Attended standup"],
            "real worklog item"
        );
    }

    #[test]
    fn test_parse_real_archive_snippet() {
        // Matches a slice of the actual Work Log Archive.md.
        let input = concat!(
            "## **Week 50**\n",
            "\n",
            "Dec 9, 2025\n",
            "\n",
            "* Made QA assignment list  \n",
            "* Sprint balancing meeting  \n",
            "\n",
            "Dec 10, 2025\n",
            "\n",
            "* Standup  \n",
            "* Checked out Mike's log level bug  \n",
            "\n",
            "## **Week 51**\n",
            "\n",
            "Dec 15, 2025\n",
            "\n",
            "* Some item\n",
        );
        let result = parse_worklog(input).unwrap();
        assert_eq!(result.weeks.len(), 2, "two weeks from archive snippet");

        let week50 = &result.weeks[0];
        assert_eq!(week50.week_number, 50);
        assert_eq!(week50.days.len(), 2, "two days in week 50");
        assert_eq!(week50.days[0].date, date(2025, 12, 9));
        assert_eq!(
            week50.days[0].items,
            vec!["Made QA assignment list", "Sprint balancing meeting"],
            "week 50 day 1 items"
        );
        assert_eq!(week50.days[1].date, date(2025, 12, 10));
        assert_eq!(
            week50.days[1].items,
            vec!["Standup", "Checked out Mike's log level bug"],
            "week 50 day 2 items"
        );

        let week51 = &result.weeks[1];
        assert_eq!(week51.week_number, 51);
        assert_eq!(week51.days[0].date, date(2025, 12, 15));
    }
}
