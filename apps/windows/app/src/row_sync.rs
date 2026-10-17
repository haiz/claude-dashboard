//! Updates a Slint `VecModel` in place from a fresh row list, so a refresh does
//! not recreate the components a `for` repeater built for unchanged rows (and
//! wipe their local state: a pending remove confirmation, an open Reset Cycles
//! list, a zoom drag). Rows are matched by key; a row whose key keeps its slot
//! gets `set_row_data` (only when it changed), and only removed, added or moved
//! rows are removed or inserted.

use slint::{Model, VecModel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Remove the model row at this index.
    Remove(usize),
    /// Insert `new[src]` at this index.
    Insert { at: usize, src: usize },
    /// The row at `at` keeps its key: overwrite it with `new[src]` if it differs.
    Set { at: usize, src: usize },
}

/// The edit script turning `old` (keys in model order) into `new`, applied in order.
pub fn plan<K: PartialEq + Clone>(old: &[K], new: &[K]) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut cur: Vec<K> = old.to_vec();
    for i in (0..cur.len()).rev() {
        if !new.contains(&cur[i]) {
            ops.push(Op::Remove(i));
            cur.remove(i);
        }
    }
    for (i, k) in new.iter().enumerate() {
        if cur.get(i) == Some(k) {
            ops.push(Op::Set { at: i, src: i });
            continue;
        }
        // Moved: take it out of its later slot, then insert it here.
        if let Some(j) = cur.iter().skip(i).position(|c| c == k) {
            ops.push(Op::Remove(i + j));
            cur.remove(i + j);
        }
        ops.push(Op::Insert { at: i, src: i });
        cur.insert(i, k.clone());
    }
    ops
}

/// Applies `new` to `model` by key (see [`plan`]).
pub fn sync<T, K>(model: &VecModel<T>, new: Vec<T>, key: impl Fn(&T) -> K)
where
    T: Clone + PartialEq + 'static,
    K: PartialEq + Clone,
{
    let old: Vec<K> = model.iter().map(|r| key(&r)).collect();
    let new_keys: Vec<K> = new.iter().map(&key).collect();
    for op in plan(&old, &new_keys) {
        match op {
            Op::Remove(i) => {
                model.remove(i);
            }
            Op::Insert { at, src } => model.insert(at, new[src].clone()),
            Op::Set { at, src } => {
                if model.row_data(at).as_ref() != Some(&new[src]) {
                    model.set_row_data(at, new[src].clone());
                }
            }
        }
    }
}

/// `new` reordered so keys already in `old` keep their old relative order and
/// keys that are new follow, in their given order. Feeds a repeater whose
/// components must survive a re-sort (the account pages).
pub fn stable_order<T, K: PartialEq>(old: &[K], mut new: Vec<T>, key: impl Fn(&T) -> K) -> Vec<T> {
    // Stable sort: new keys (no old position) keep their order after the old ones.
    new.sort_by_key(|r| {
        let k = key(r);
        old.iter().position(|o| *o == k).unwrap_or(usize::MAX)
    });
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &[&str], new: &[&str]) -> Vec<String> {
        let mut cur: Vec<String> = old.iter().map(|s| s.to_string()).collect();
        for op in plan(old, new) {
            match op {
                Op::Remove(i) => {
                    cur.remove(i);
                }
                Op::Insert { at, src } => cur.insert(at, new[src].to_string()),
                Op::Set { at, src } => assert_eq!(cur[at], new[src]),
            }
        }
        cur
    }

    #[test]
    fn same_order_is_only_sets() {
        let ops = plan(&["a", "b", "c"], &["a", "b", "c"]);
        assert!(ops.iter().all(|o| matches!(o, Op::Set { .. })), "{ops:?}");
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn plans_reach_the_new_order() {
        let cases: [(&[&str], &[&str]); 7] = [
            (&[], &["a", "b"]),
            (&["a", "b"], &[]),
            (&["a", "b", "c"], &["a", "c"]),
            (&["a", "c"], &["a", "b", "c"]),
            (&["a", "b", "c"], &["c", "a", "b"]),
            (&["a", "b", "c"], &["b", "a", "d"]),
            (&["x", "a"], &["a", "y"]),
        ];
        for (old, new) in cases {
            assert_eq!(apply(old, new), new, "{old:?} -> {new:?}");
        }
    }

    #[test]
    fn removal_keeps_the_rest_as_sets() {
        let ops = plan(&["a", "b", "c"], &["a", "c"]);
        assert_eq!(
            ops,
            vec![
                Op::Remove(1),
                Op::Set { at: 0, src: 0 },
                Op::Set { at: 1, src: 1 }
            ]
        );
    }

    #[test]
    fn sync_updates_a_vec_model_in_place() {
        let m = VecModel::from(vec![("a", 1), ("b", 2), ("c", 3)]);
        sync(&m, vec![("a", 1), ("b", 20), ("c", 3)], |r| r.0);
        assert_eq!(
            m.iter().collect::<Vec<_>>(),
            [("a", 1), ("b", 20), ("c", 3)]
        );
        sync(&m, vec![("c", 3), ("d", 4), ("a", 1)], |r| r.0);
        assert_eq!(m.iter().collect::<Vec<_>>(), [("c", 3), ("d", 4), ("a", 1)]);
    }

    #[test]
    fn stable_order_keeps_old_positions_and_appends_new() {
        let out = stable_order(&["a", "b", "c"], vec!["c", "z", "a", "b"], |s| *s);
        assert_eq!(out, ["a", "b", "c", "z"]);
        let out = stable_order(&["a", "b", "c"], vec!["b", "a"], |s| *s);
        assert_eq!(out, ["a", "b"]);
    }
}
