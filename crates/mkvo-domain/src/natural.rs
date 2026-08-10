use std::cmp::Ordering;

/// Compare strings case-insensitively while ordering embedded ASCII digit runs
/// by numeric value without parsing them into a fixed-width integer.
///
/// Equal numeric values continue comparing later segments; if the complete
/// strings otherwise compare equally, the full UTF-16 length is the tie-breaker.
/// This preserves the behavior of MKVO's legacy `.NET` natural comparer,
/// including `1 < 01 < 001`.
#[must_use]
pub fn natural_compare(left: &str, right: &str) -> Ordering {
    let left_chars: Vec<_> = left.chars().collect();
    let right_chars: Vec<_> = right.chars().collect();
    let mut left_index = 0;
    let mut right_index = 0;

    while left_index < left_chars.len() && right_index < right_chars.len() {
        if left_chars[left_index].is_ascii_digit() && right_chars[right_index].is_ascii_digit() {
            let left_start = left_index;
            let right_start = right_index;
            while left_index < left_chars.len() && left_chars[left_index].is_ascii_digit() {
                left_index += 1;
            }
            while right_index < right_chars.len() && right_chars[right_index].is_ascii_digit() {
                right_index += 1;
            }

            let left_number = significant_digits(&left_chars[left_start..left_index]);
            let right_number = significant_digits(&right_chars[right_start..right_index]);
            match left_number.len().cmp(&right_number.len()) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
            match left_number.cmp(right_number) {
                Ordering::Equal => continue,
                ordering => return ordering,
            }
        }

        let left_upper = left_chars[left_index]
            .to_uppercase()
            .next()
            .unwrap_or(left_chars[left_index]);
        let right_upper = right_chars[right_index]
            .to_uppercase()
            .next()
            .unwrap_or(right_chars[right_index]);
        match left_upper.cmp(&right_upper) {
            Ordering::Equal => {
                left_index += 1;
                right_index += 1;
            }
            ordering => return ordering,
        }
    }

    left.encode_utf16()
        .count()
        .cmp(&right.encode_utf16().count())
}

/// Sort owned strings using [`natural_compare`].
pub fn sort_natural(values: &mut [String]) {
    values.sort_by(|left, right| natural_compare(left, right));
}

fn significant_digits(digits: &[char]) -> &[char] {
    let first_nonzero = digits.iter().position(|digit| *digit != '0');
    first_nonzero.map_or_else(
        || &digits[digits.len().saturating_sub(1)..],
        |index| &digits[index..],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct NaturalSortFixture {
        cases: Vec<NaturalSortCase>,
    }

    #[derive(serde::Deserialize)]
    struct NaturalSortCase {
        id: String,
        input: Vec<String>,
        expected: Vec<String>,
    }

    fn assert_order(input: &[&str], expected: &[&str]) {
        let mut values = input
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        sort_natural(&mut values);
        assert_eq!(values, expected);
    }

    #[test]
    fn embedded_episode_numbers() {
        assert_order(
            &["Episode 10", "Episode 2", "Episode 1"],
            &["Episode 1", "Episode 2", "Episode 10"],
        );
    }

    #[test]
    fn leading_zero_tie_break_uses_full_length() {
        assert_order(
            &[
                "Episode 001",
                "Episode 1",
                "Episode 000",
                "Episode 00",
                "Episode 01",
                "Episode 10",
                "Episode 2",
            ],
            &[
                "Episode 00",
                "Episode 000",
                "Episode 1",
                "Episode 01",
                "Episode 001",
                "Episode 2",
                "Episode 10",
            ],
        );
    }

    #[test]
    fn compares_multiple_numeric_segments() {
        assert_order(
            &[
                "Show S2E10 Part 1",
                "Show S10E1 Part 1",
                "Show S2E2 Part 10",
                "Show S2E2 Part 2",
                "Show S2E2",
            ],
            &[
                "Show S2E2",
                "Show S2E2 Part 2",
                "Show S2E2 Part 10",
                "Show S2E10 Part 1",
                "Show S10E1 Part 1",
            ],
        );
    }

    #[test]
    fn arbitrarily_large_numbers_do_not_overflow() {
        assert_order(
            &[
                "Episode 100000000000000000000",
                "Episode 99999999999999999999",
                "Episode 20",
            ],
            &[
                "Episode 20",
                "Episode 99999999999999999999",
                "Episode 100000000000000000000",
            ],
        );
    }

    #[test]
    fn executes_every_natural_sort_fixture_case() {
        let fixture: NaturalSortFixture = serde_json::from_str(include_str!(
            "../../../tests/parity-fixtures/natural-sort.json"
        ))
        .expect("natural-sort fixture JSON");

        assert_eq!(fixture.cases.len(), 4, "fixture case count changed");
        for mut case in fixture.cases {
            sort_natural(&mut case.input);
            assert_eq!(case.input, case.expected, "fixture case `{}`", case.id);
        }
    }
}
