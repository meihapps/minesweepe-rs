use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::game::generate_mines;
use crate::types::{Board, GameConfig};

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct Job {
    first_click: (u16, u16),
    config: GameConfig,
    generation_id: u64,
}

struct Result {
    first_click: (u16, u16),
    generation_id: u64,
    board: Board,
}

struct Shared {
    job: Mutex<Option<Job>>,
    wake: Condvar,
    generation: AtomicU64,
    result: Mutex<Option<Result>>,
}

// ---------------------------------------------------------------------------
// Public handle
// ---------------------------------------------------------------------------

/// Owns a background worker thread that pre-generates solvable boards.
/// Call `submit` on hover to kick off generation for a given first-click cell.
/// Call `take` on the actual click to retrieve the board if it's ready.
pub struct PregenHandle {
    shared: Arc<Shared>,
}

impl PregenHandle {
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            job: Mutex::new(None),
            wake: Condvar::new(),
            generation: AtomicU64::new(0),
            result: Mutex::new(None),
        });
        let worker_shared = shared.clone();
        std::thread::Builder::new()
            .name("pregen-worker".into())
            .spawn(move || worker(worker_shared))
            .expect("failed to spawn pregen worker");
        PregenHandle { shared }
    }

    /// Submit a new generation job for `first_click`. Cancels any in-progress job.
    pub fn submit(&self, first_click: (u16, u16), config: GameConfig) {
        let gen = self.shared.generation.fetch_add(1, Ordering::Release) + 1;
        *self.shared.result.lock().unwrap() = None;
        *self.shared.job.lock().unwrap() = Some(Job { first_click, config, generation_id: gen });
        self.shared.wake.notify_one();
    }

    /// Returns the ready board if it was generated for `first_click` and is still current.
    /// Takes ownership of the result, clearing it.
    pub fn take(&self, first_click: (u16, u16)) -> Option<Board> {
        let current_gen = self.shared.generation.load(Ordering::Acquire);
        let mut slot = self.shared.result.lock().unwrap();
        match slot.as_ref() {
            Some(r) if r.first_click == first_click && r.generation_id == current_gen => {
                Some(slot.take().unwrap().board)
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

fn worker(shared: Arc<Shared>) {
    loop {
        let job = {
            let mut slot = shared.job.lock().unwrap();
            loop {
                match slot.take() {
                    Some(j) => break j,
                    None => slot = shared.wake.wait(slot).unwrap(),
                }
            }
        };

        if let Some(board) = generate(&job, &shared.generation) {
            *shared.result.lock().unwrap() = Some(Result {
                first_click: job.first_click,
                generation_id: job.generation_id,
                board,
            });
        }
    }
}

fn generate(job: &Job, generation: &AtomicU64) -> Option<Board> {
    let config = &job.config;
    let mut board = Board::new(config.width, config.height);
    let cancelled = || generation.load(Ordering::Relaxed) != job.generation_id;
    generate_mines(&mut board, config, job.first_click, cancelled).then_some(board)
}
