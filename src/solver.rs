use std::collections::{HashMap, VecDeque};
use rayon::prelude::*;
use crate::types::Board;

#[derive(PartialEq, Eq)]
pub enum SolveResult {
    Solved,
    Unsolvable,
}

/// All logically deducible cells sharing the same witnesses.
pub struct Deduction {
    pub mine_cells: Vec<(u16, u16)>,
    pub safe_cells: Vec<(u16, u16)>,
    /// Revealed cells whose constraints are sufficient to force the conclusion.
    pub witnesses: Vec<(u16, u16)>,
    /// True if the global mine counter was required (Tier 2 hint).
    pub uses_global: bool,
}

impl Deduction {
    fn total_cells(&self) -> usize { self.mine_cells.len() + self.safe_cells.len() }
    /// Fewest witnesses → most cells → more mines (tiebreaker).
    fn beats(&self, other: &Deduction) -> bool {
        if self.witnesses.len() != other.witnesses.len() {
            return self.witnesses.len() < other.witnesses.len();
        }
        if self.total_cells() != other.total_cells() {
            return self.total_cells() > other.total_cells();
        }
        self.mine_cells.len() > other.mine_cells.len()
    }
}

// ── Internal types ──────────────────────────────────────────────────────────

struct Constraint {
    witness: usize,       // board index of the revealed numbered cell
    cells: Vec<usize>,    // frontier (unrevealed, unflagged) neighbour indices
    count: usize,         // remaining mines in `cells`
}

struct ComponentAnalysis {
    frontier: Vec<usize>,             // ordered board indices
    configs: Vec<(Vec<bool>, usize)>, // (assignment per frontier cell, mine count)
    constraint_indices: Vec<usize>,   // original indices into the constraints slice
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn flood_fill(board: &Board, start: usize, revealed: &mut [bool]) {
    let mut q = VecDeque::new();
    q.push_back(start);
    while let Some(idx) = q.pop_front() {
        if revealed[idx] || board.cells[idx].is_mine { continue; }
        revealed[idx] = true;
        if board.cells[idx].adjacent_mines == 0 {
            let x = (idx as u16) % board.width;
            let y = (idx as u16) / board.width;
            for (nx, ny) in board.neighbours(x, y) {
                let ni = board.idx(nx, ny);
                if !revealed[ni] { q.push_back(ni); }
            }
        }
    }
}

fn build_constraints(board: &Board, revealed: &[bool], flagged: &[bool]) -> Vec<Constraint> {
    let mut result = Vec::new();
    for y in 0..board.height {
        for x in 0..board.width {
            let idx = board.idx(x, y);
            if !revealed[idx] { continue; }
            let adj = board.cells[idx].adjacent_mines as usize;
            if adj == 0 { continue; }

            let mut cells = Vec::new();
            let mut known_mines = 0usize;
            for (nx, ny) in board.neighbours(x, y) {
                let ni = board.idx(nx, ny);
                if flagged[ni] {
                    known_mines += 1;
                } else if !revealed[ni] {
                    cells.push(ni);
                }
            }
            if known_mines > adj { continue; }
            let count = adj - known_mines;
            if !cells.is_empty() {
                result.push(Constraint { witness: idx, cells, count });
            }
        }
    }
    result
}

fn analyse_components(constraints: &[Constraint]) -> Vec<ComponentAnalysis> {
    if constraints.is_empty() { return vec![]; }

    // Map each frontier cell to the constraints that contain it.
    let mut cell_to_constraints: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, c) in constraints.iter().enumerate() {
        for &cell in &c.cells {
            cell_to_constraints.entry(cell).or_default().push(i);
        }
    }

    // BFS over constraints to find connected components.
    let n = constraints.len();
    let mut visited = vec![false; n];
    let mut component_groups: Vec<Vec<usize>> = Vec::new();

    for start in 0..n {
        if visited[start] { continue; }
        let mut group = Vec::new();
        let mut q = VecDeque::new();
        q.push_back(start);
        visited[start] = true;
        while let Some(ci) = q.pop_front() {
            group.push(ci);
            for &cell in &constraints[ci].cells {
                if let Some(others) = cell_to_constraints.get(&cell) {
                    for &other in others {
                        if !visited[other] {
                            visited[other] = true;
                            q.push_back(other);
                        }
                    }
                }
            }
        }
        component_groups.push(group);
    }

    // Enumerate valid mine assignments for each component.
    component_groups.iter().map(|group| {
        let mut frontier: Vec<usize> = group.iter()
            .flat_map(|&ci| constraints[ci].cells.iter().copied())
            .collect();
        frontier.sort_unstable();
        frontier.dedup();

        let cell_to_local: HashMap<usize, usize> = frontier.iter().enumerate()
            .map(|(i, &c)| (c, i))
            .collect();

        let local_constraints: Vec<(Vec<usize>, usize)> = group.iter()
            .map(|&ci| {
                let c = &constraints[ci];
                let local: Vec<usize> = c.cells.iter().map(|&f| cell_to_local[&f]).collect();
                (local, c.count)
            })
            .collect();

        let mut configs: Vec<(Vec<bool>, usize)> = Vec::new();
        let mut assignment = vec![false; frontier.len()];
        backtrack(&local_constraints, &mut assignment, 0, 0, 0, usize::MAX, &mut configs);

        ComponentAnalysis { frontier, configs, constraint_indices: group.clone() }
    }).collect()
}

/// `k_min`/`k_max` act as a global mine-count constraint over the whole assignment:
/// treat them as a virtual constraint "total mines in all cells = [k_min, k_max]".
fn backtrack(
    constraints: &[(Vec<usize>, usize)],
    assignment: &mut Vec<bool>,
    pos: usize,
    mines_so_far: usize,
    k_min: usize,
    k_max: usize,
    valid: &mut Vec<(Vec<bool>, usize)>,
) {
    // Prune on total mine count (global constraint).
    if mines_so_far > k_max { return; }
    if mines_so_far + (assignment.len() - pos) < k_min { return; }

    // Prune on local constraints.
    for (cells, count) in constraints {
        let (mines, unknowns) = cells.iter().fold((0usize, 0usize), |(m, u), &li| {
            if li < pos { (m + assignment[li] as usize, u) } else { (m, u + 1) }
        });
        if mines > *count { return; }
        if mines + unknowns < *count { return; }
    }

    if pos == assignment.len() {
        if mines_so_far >= k_min {
            valid.push((assignment.clone(), mines_so_far));
        }
        return;
    }

    for &mine in &[false, true] {
        assignment[pos] = mine;
        backtrack(constraints, assignment, pos + 1,
                  mines_so_far + mine as usize, k_min, k_max, valid);
    }
}

/// Builds the active frontier from the selected constraints' cells, locates
/// `target_idx` within it, and runs backtrack. Only cells referenced by the
/// selected constraints are enumerated; cells in the wider component frontier
/// that no selected constraint touches are free and don't affect forcing.
/// Returns `None` if the target isn't in the active frontier or no configs exist.
/// Builds the active frontier from the selected constraints, runs backtrack with
/// the given mine-count bounds (k_min=0, k_max=usize::MAX for local/unconstrained;
/// tighter bounds encode the global mine-count constraint).
fn make_local_constraints(
    included: &[usize],
    frontier: &[usize],
    constraints: &[Constraint],
) -> Vec<(Vec<usize>, usize)> {
    included.iter()
        .map(|&ci| {
            let c = &constraints[ci];
            let local: Vec<usize> = c.cells.iter()
                .filter_map(|&cell| frontier.binary_search(&cell).ok())
                .collect();
            (local, c.count)
        })
        .collect()
}

/// Evaluates a single combo: runs backtrack once, then checks every candidate
/// in `forced` against the resulting configs. Phase 2 only — no global constraint.
fn eval_combo(
    included: &[usize],
    forced: &[(usize, bool)],
    constraints: &[Constraint],
    board: &Board,
) -> Option<Deduction> {
    let mut frontier: Vec<usize> = included.iter()
        .flat_map(|&ci| constraints[ci].cells.iter().copied())
        .collect();
    frontier.sort_unstable();
    frontier.dedup();

    let lc = make_local_constraints(included, &frontier, constraints);
    let mut configs = Vec::new();
    backtrack(&lc, &mut vec![false; frontier.len()], 0, 0, 0, usize::MAX, &mut configs);
    if configs.is_empty() { return None; }

    let mut mine_cells: Vec<(u16, u16)> = Vec::new();
    let mut safe_cells: Vec<(u16, u16)> = Vec::new();

    for &(bi, is_mine) in forced {
        if let Ok(tl) = frontier.binary_search(&bi) {
            let is_forced = if is_mine { configs.iter().all(|(cfg, _)| cfg[tl]) }
                            else       { configs.iter().all(|(cfg, _)| !cfg[tl]) };
            if is_forced {
                let x = (bi as u16) % board.width;
                let y = (bi as u16) / board.width;
                if is_mine { mine_cells.push((x, y)); }
                else       { safe_cells.push((x, y)); }
            }
        }
    }

    if mine_cells.is_empty() && safe_cells.is_empty() { return None; }

    let witnesses: Vec<(u16, u16)> = included.iter()
        .map(|&ci| {
            let wi = constraints[ci].witness;
            ((wi as u16) % board.width, (wi as u16) / board.width)
        })
        .collect();

    Some(Deduction { mine_cells, safe_cells, witnesses, uses_global: false })
}

/// Returns true if constraints `a` and `b` share any frontier cell.
fn constraints_overlap(a: &Constraint, b: &Constraint) -> bool {
    a.cells.iter().any(|&c| b.cells.contains(&c))
}

/// Finds the subset of `constraints` that are pairwise non-overlapping and
/// whose combined cell count is maximised. Uses backtracking with an
/// upper-bound prune on the suffix sum of sorted cell counts.
fn max_nonoverlapping_coverage(constraints: &[Constraint]) -> Vec<usize> {
    let n = constraints.len();
    if n == 0 { return vec![]; }

    // Process in descending cell-count order for tighter pruning.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_unstable_by_key(|&i| std::cmp::Reverse(constraints[i].cells.len()));

    // suffix[i] = upper bound on additional coverage from order[i..].
    let mut suffix = vec![0usize; n + 1];
    for i in (0..n).rev() {
        suffix[i] = suffix[i + 1] + constraints[order[i]].cells.len();
    }

    let mut best = 0usize;
    let mut best_set: Vec<usize> = Vec::new();
    let mut current: Vec<usize> = Vec::new();

    nonoverlap_bt(0, &order, &mut current, 0, constraints, &suffix, &mut best, &mut best_set);
    best_set
}

fn nonoverlap_bt(
    pos: usize,
    order: &[usize],
    current: &mut Vec<usize>,
    coverage: usize,
    constraints: &[Constraint],
    suffix: &[usize],
    best: &mut usize,
    best_set: &mut Vec<usize>,
) {
    if coverage > *best {
        *best = coverage;
        *best_set = current.clone();
    }
    // Even adding every remaining constraint can't beat current best — prune.
    if coverage + suffix[pos] <= *best { return; }

    for i in pos..order.len() {
        let ci = order[i];
        if current.iter().any(|&cj| constraints_overlap(&constraints[ci], &constraints[cj])) {
            continue;
        }
        current.push(ci);
        nonoverlap_bt(i + 1, order, current, coverage + constraints[ci].cells.len(),
                      constraints, suffix, best, best_set);
        current.pop();
    }
}

/// Generates all k-combinations of indices 0..n.
fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    if k == 0 || k > n { return vec![]; }
    let mut result = Vec::new();
    let mut combo: Vec<usize> = (0..k).collect();
    loop {
        result.push(combo.clone());
        let mut i = k;
        loop {
            if i == 0 { return result; }
            i -= 1;
            if combo[i] < n - (k - i) {
                combo[i] += 1;
                for j in (i + 1)..k { combo[j] = combo[j - 1] + 1; }
                break;
            }
        }
    }
}

/// Sweeps k-combinations of constraint indices (size 1..=max_size) to find the
/// smallest witness subset that forces any cell in `forced`. Returns the best
/// `Deduction` found at the first size tier that produces any result, or `None`.
///
/// `first_only`: early-exit as soon as any deduction is found (solve path).
/// `!first_only`: exhaustive search at each size, returning the best (hint path).
fn min_witness_subset(
    constraint_indices: &[usize],
    forced: &[(usize, bool)],
    constraints: &[Constraint],
    board: &Board,
    max_size: usize,
    first_only: bool,
) -> Option<Deduction> {
    let n = constraint_indices.len();

    let eval = |combo: &Vec<usize>| -> Option<Deduction> {
        let included: Vec<usize> = combo.iter().map(|&i| constraint_indices[i]).collect();
        eval_combo(&included, forced, constraints, board)
    };

    const PAR_THRESHOLD: usize = 32;

    for size in 1..=max_size.min(n) {
        let combos = combinations(n, size);
        let parallel = combos.len() >= PAR_THRESHOLD;
        if first_only {
            let found = if parallel {
                combos.par_iter().find_map_any(|c| eval(c))
            } else {
                combos.iter().find_map(|c| eval(c))
            };
            if found.is_some() { return found; }
        } else {
            let best = if parallel {
                combos.par_iter()
                    .filter_map(|c| eval(c))
                    .reduce_with(|a, b| if a.beats(&b) { a } else { b })
            } else {
                combos.iter()
                    .filter_map(|c| eval(c))
                    .reduce(|a, b| if a.beats(&b) { a } else { b })
            };
            if best.is_some() { return best; }
        }
    }

    None
}

// ── Public API ───────────────────────────────────────────────────────────────

pub fn solve(board: &Board, first_click: (u16, u16)) -> SolveResult {
    let total_mines = board.cells.iter().filter(|c| c.is_mine).count() as u16;
    let n = board.cells.len();
    let mut revealed = vec![false; n];
    let mut flagged = vec![false; n];
    let mut flags_placed: u16 = 0;

    flood_fill(board, board.idx(first_click.0, first_click.1), &mut revealed);

    loop {
        if board.cells.iter().enumerate().all(|(i, c)| c.is_mine || revealed[i]) {
            return SolveResult::Solved;
        }

        let Some(d) = hint_inner(board, &revealed, &flagged, flags_placed, total_mines, true) else {
            return SolveResult::Unsolvable;
        };

        for (x, y) in d.mine_cells {
            let idx = board.idx(x, y);
            flagged[idx] = true;
            flags_placed += 1;
        }
        for (x, y) in d.safe_cells {
            flood_fill(board, board.idx(x, y), &mut revealed);
        }
    }
}

pub fn hint(
    board: &Board,
    revealed: &[bool],
    flagged: &[bool],
    flags_placed: u16,
    total_mines: u16,
) -> Option<Deduction> {
    hint_inner(board, revealed, flagged, flags_placed, total_mines, false)
}

/// Phase 1 (trivial):  a single constraint is entirely mine or entirely safe.
/// Phase 2 (complex):  ≤3 local constraints force a cell via backtracking.
/// Phase 3 (global):   non-overlapping constraints maximise coverage; the
///                     global mine count then forces the uncovered passive region.
///
/// `first_only`: return immediately on the first deduction found (solve path).
fn hint_inner(
    board: &Board,
    revealed: &[bool],
    flagged: &[bool],
    flags_placed: u16,
    total_mines: u16,
    first_only: bool,
) -> Option<Deduction> {
    let constraints = build_constraints(board, revealed, flagged);

    // ── Phase 1 ───────────────────────────────────────────────────────────
    {
        let mut best: Option<Deduction> = None;
        for c in &constraints {
            let all_mine = c.count > 0 && c.count == c.cells.len();
            let all_safe = c.count == 0;
            if !all_mine && !all_safe { continue; }
            let wx = (c.witness as u16) % board.width;
            let wy = (c.witness as u16) / board.width;
            let mut mine_cells = Vec::new();
            let mut safe_cells = Vec::new();
            for &bi in &c.cells {
                if revealed[bi] || flagged[bi] { continue; }
                let x = (bi as u16) % board.width;
                let y = (bi as u16) / board.width;
                if all_mine { mine_cells.push((x, y)); }
                else        { safe_cells.push((x, y)); }
            }
            if mine_cells.is_empty() && safe_cells.is_empty() { continue; }
            let d = Deduction { mine_cells, safe_cells, witnesses: vec![(wx, wy)], uses_global: false };
            if first_only { return Some(d); }
            if best.as_ref().map_or(true, |b| d.beats(b)) { best = Some(d); }
        }
        if best.is_some() { return best; }
    }

    // ── Shared setup for Phases 2 & 3 ────────────────────────────────────
    let components = analyse_components(&constraints);
    let n = board.cells.len();
    let remaining = (total_mines as usize).saturating_sub(flags_placed as usize);

    let all_unrevealed: Vec<usize> = (0..n)
        .filter(|&i| !revealed[i] && !flagged[i])
        .collect();

    // ── Phase 2 ───────────────────────────────────────────────────────────
    // Up to 3 local witnesses per component; strictly local, no global count.
    {
        const MAX_WITNESSES: usize = 3;
        let mut best: Option<Deduction> = None;
        for ca in &components {
            if ca.configs.is_empty() { continue; }

            let candidates: Vec<(usize, bool)> = ca.frontier.iter()
                .filter(|&&bi| !revealed[bi] && !flagged[bi])
                .flat_map(|&bi| [(bi, true), (bi, false)])
                .collect();
            if candidates.is_empty() { continue; }

            let comp_best = min_witness_subset(
                &ca.constraint_indices, &candidates, &constraints, board,
                MAX_WITNESSES, first_only,
            );
            if let Some(d) = comp_best {
                if first_only { return Some(d); }
                if best.as_ref().map_or(true, |b| d.beats(b)) { best = Some(d); }
            }
        }
        if best.is_some() { return best; }
    }

    // ── Phase 3 ───────────────────────────────────────────────────────────
    // Find the set of pairwise non-overlapping constraints with maximum total
    // cell coverage. The global mine count then determines how many mines fall
    // in the uncovered (passive) region; if that count is 0 or equals the
    // passive cell count, every passive cell is forced.
    {
        let selected = max_nonoverlapping_coverage(&constraints);

        let covered_mines: usize = selected.iter().map(|&ci| constraints[ci].count).sum();
        if remaining < covered_mines { return None; }

        let mut covered: Vec<usize> = selected.iter()
            .flat_map(|&ci| constraints[ci].cells.iter().copied())
            .collect();
        covered.sort_unstable();
        covered.dedup();

        let passive: Vec<usize> = all_unrevealed.iter()
            .filter(|&&c| covered.binary_search(&c).is_err())
            .copied()
            .collect();

        if passive.is_empty() { return None; }

        let passive_mines = remaining - covered_mines;

        let (mine_cells, safe_cells) = if passive_mines == 0 {
            let safe = passive.iter()
                .map(|&bi| ((bi as u16) % board.width, (bi as u16) / board.width))
                .collect();
            (vec![], safe)
        } else if passive_mines == passive.len() {
            let mines = passive.iter()
                .map(|&bi| ((bi as u16) % board.width, (bi as u16) / board.width))
                .collect();
            (mines, vec![])
        } else {
            return None;
        };

        let witnesses: Vec<(u16, u16)> = selected.iter()
            .map(|&ci| {
                let wi = constraints[ci].witness;
                ((wi as u16) % board.width, (wi as u16) / board.width)
            })
            .collect();

        Some(Deduction { mine_cells, safe_cells, witnesses, uses_global: true })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::place_mines;
    use crate::types::{Board, Difficulty};

    fn run_n_games(difficulty: Difficulty, n: usize) -> (usize, std::time::Duration) {
        let config = difficulty.config();
        let first_click = (config.width / 2, config.height / 2);
        let mut solved = 0;
        let start = std::time::Instant::now();
        for _ in 0..n {
            let mut board = Board::new(config.width, config.height);
            place_mines(&mut board, &config, first_click);
            if solve(&board, first_click) == SolveResult::Solved {
                solved += 1;
            }
        }
        (solved, start.elapsed())
    }

    #[test]
    fn all_generated_boards_are_solvable() {
        for difficulty in [Difficulty::Beginner, Difficulty::Intermediate, Difficulty::Expert] {
            let (solved, elapsed) = run_n_games(difficulty, 50);
            let label = difficulty.label();
            let per_game = elapsed / 50;
            println!("{label}: {solved}/50 solvable, {elapsed:?} total ({per_game:?}/game)");
            assert_eq!(solved, 50, "{label}: not all generated boards were solvable");
        }
    }

    #[test]
    fn profile_expert() {
        let (solved, elapsed) = run_n_games(Difficulty::Expert, 5);
        println!("Expert: {solved}/5 solvable in {elapsed:?}");
    }
}
