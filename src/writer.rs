use crate::parser::WorkLog;

/// Write structured worklog data back to canonical markdown format.
///
/// The output uses plain (non-bold) week headers and the `%b %-d, %Y` date
/// format (e.g. `Mar 9, 2026`), matching the format consumed by `parse_worklog`.
pub fn write_worklog(worklog: &WorkLog) -> String {
    let mut out = String::new();

    for (week_idx, week) in worklog.weeks.iter().enumerate() {
        if week_idx > 0 {
            out.push('\n');
        }

        out.push_str(&format!("## Week {}\n", week.week_number));

        for day in &week.days {
            // Blank line between week header (or previous day's items) and the
            // date line.
            out.push('\n');

            let date_str = day.date.format("%b %-d, %Y").to_string();
            out.push_str(&date_str);
            out.push('\n');

            // Blank line between date line and first item.
            out.push('\n');

            for item in &day.items {
                out.push_str(&format!("* {item}\n"));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DayEntry, WeekEntry, parse_worklog};
    use chrono::NaiveDate;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    // --- Error / edge cases ---

    #[test]
    fn test_write_empty_worklog() {
        let worklog = WorkLog { weeks: vec![] };
        let result = write_worklog(&worklog);
        assert_eq!(result, "", "empty worklog should produce empty string");
    }

    // --- Trivial cases ---

    #[test]
    fn test_write_single_week_no_days() {
        let worklog = WorkLog {
            weeks: vec![WeekEntry {
                week_number: 1,
                iso_year: None,
                days: vec![],
            }],
        };
        let result = write_worklog(&worklog);
        assert_eq!(
            result, "## Week 1\n",
            "single week with no days produces header only"
        );
    }

    #[test]
    fn test_write_single_week_one_day_one_item() {
        let worklog = WorkLog {
            weeks: vec![WeekEntry {
                week_number: 1,
                iso_year: None,
                days: vec![DayEntry {
                    date: date(2026, 1, 5),
                    items: vec!["Did a thing".to_string()],
                }],
            }],
        };
        let result = write_worklog(&worklog);
        let expected = "## Week 1\n\nJan 5, 2026\n\n* Did a thing\n";
        assert_eq!(result, expected, "single week, one day, one item");
    }

    #[test]
    fn test_write_single_day_with_single_digit_date() {
        // %-d produces no padding so Mar 9 not Mar  9.
        let worklog = WorkLog {
            weeks: vec![WeekEntry {
                week_number: 11,
                iso_year: None,
                days: vec![DayEntry {
                    date: date(2026, 3, 9),
                    items: vec!["Attended standup".to_string()],
                }],
            }],
        };
        let result = write_worklog(&worklog);
        let expected = "## Week 11\n\nMar 9, 2026\n\n* Attended standup\n";
        assert_eq!(result, expected, "single-digit day must not be zero-padded");
    }

    #[test]
    fn test_write_single_week_one_day_multiple_items() {
        let worklog = WorkLog {
            weeks: vec![WeekEntry {
                week_number: 5,
                iso_year: None,
                days: vec![DayEntry {
                    date: date(2026, 2, 2),
                    items: vec![
                        "Item one".to_string(),
                        "Item two".to_string(),
                        "Item three".to_string(),
                    ],
                }],
            }],
        };
        let result = write_worklog(&worklog);
        let expected = "## Week 5\n\nFeb 2, 2026\n\n* Item one\n* Item two\n* Item three\n";
        assert_eq!(result, expected, "single week, one day, multiple items");
    }

    #[test]
    fn test_write_single_week_multiple_days() {
        let worklog = WorkLog {
            weeks: vec![WeekEntry {
                week_number: 10,
                iso_year: None,
                days: vec![
                    DayEntry {
                        date: date(2026, 3, 2),
                        items: vec!["Morning standup".to_string()],
                    },
                    DayEntry {
                        date: date(2026, 3, 3),
                        items: vec!["Code review".to_string()],
                    },
                ],
            }],
        };
        let result = write_worklog(&worklog);
        let expected = concat!(
            "## Week 10\n",
            "\n",
            "Mar 2, 2026\n",
            "\n",
            "* Morning standup\n",
            "\n",
            "Mar 3, 2026\n",
            "\n",
            "* Code review\n",
        );
        assert_eq!(result, expected, "single week with multiple days");
    }

    #[test]
    fn test_write_multiple_weeks() {
        let worklog = WorkLog {
            weeks: vec![
                WeekEntry {
                    week_number: 1,
                    iso_year: None,
                    days: vec![DayEntry {
                        date: date(2026, 1, 5),
                        items: vec!["Item A".to_string()],
                    }],
                },
                WeekEntry {
                    week_number: 2,
                    iso_year: None,
                    days: vec![DayEntry {
                        date: date(2026, 1, 12),
                        items: vec!["Item B".to_string()],
                    }],
                },
            ],
        };
        let result = write_worklog(&worklog);
        let expected = concat!(
            "## Week 1\n",
            "\n",
            "Jan 5, 2026\n",
            "\n",
            "* Item A\n",
            "\n",
            "## Week 2\n",
            "\n",
            "Jan 12, 2026\n",
            "\n",
            "* Item B\n",
        );
        assert_eq!(result, expected, "multiple weeks separated by blank line");
    }

    // --- Round-trip ---

    #[test]
    fn test_round_trip_parse_write_parse() {
        // parse → write → parse must yield identical structured data.
        let original = concat!(
            "## Week 1\n",
            "\n",
            "Jan 5, 2026\n",
            "\n",
            "* Item A\n",
            "\n",
            "## Week 2\n",
            "\n",
            "Jan 12, 2026\n",
            "\n",
            "* Item B\n",
            "* Item C\n",
        );
        let parsed_first = parse_worklog(original).expect("first parse should succeed");
        let written = write_worklog(&parsed_first);
        let parsed_second = parse_worklog(&written).expect("second parse should succeed");
        assert_eq!(
            parsed_first, parsed_second,
            "round-trip parse→write→parse must preserve all data"
        );
    }
}
