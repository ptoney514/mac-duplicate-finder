//! Gap-based event grouping (PRD §9.5): photos sorted by capture time break
//! into a new event whenever the gap to the previous photo exceeds the
//! threshold (default 2 hours).

pub const DEFAULT_EVENT_GAP_SECS: i64 = 2 * 60 * 60;

/// Groups (id, captured_at) pairs into events. Every dated photo lands in
/// exactly one group (singletons included); ids keep chronological order
/// within a group; groups come back chronological.
pub fn gap_groups(times: &[(i64, i64)], gap_secs: i64) -> Vec<Vec<i64>> {
    let mut sorted: Vec<(i64, i64)> = times.to_vec();
    sorted.sort_by_key(|&(id, at)| (at, id));

    let mut groups: Vec<Vec<i64>> = Vec::new();
    let mut previous: Option<i64> = None;
    for (id, at) in sorted {
        match previous {
            Some(prev) if at - prev <= gap_secs => {
                groups.last_mut().expect("group exists").push(id)
            }
            _ => groups.push(vec![id]),
        }
        previous = Some(at);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_gaps_and_keeps_chronology() {
        // Morning event, then a 3h gap, then an afternoon event, plus a
        // lone evening shot. Input deliberately unsorted.
        let times = [
            (3, 11_000),
            (1, 10_000),
            (2, 10_500),
            (4, 25_000),
            (5, 25_400),
            (6, 60_000),
        ];
        let groups = gap_groups(&times, DEFAULT_EVENT_GAP_SECS);
        assert_eq!(groups, vec![vec![1, 2, 3], vec![4, 5], vec![6]]);
    }

    #[test]
    fn boundary_gap_stays_in_the_same_event() {
        let gap = DEFAULT_EVENT_GAP_SECS;
        let times = [(1, 0), (2, gap), (3, 2 * gap + 1)];
        let groups = gap_groups(&times, gap);
        assert_eq!(groups, vec![vec![1, 2], vec![3]]);
    }

    #[test]
    fn empty_input_gives_no_groups() {
        assert!(gap_groups(&[], DEFAULT_EVENT_GAP_SECS).is_empty());
    }
}
