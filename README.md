# minesweepe-rs

rust minesweeper. minesweeper in rust.

![demo](demo.gif)

## install

```
cargo install minesweepe-rs
```

## controls

| action | keyboard | mouse |
|--------|----------|-------|
| move cursor | `hjkl` / arrow keys | — |
| reveal | `enter` / `space` | left-click |
| flag | `f` | right-click |
| reveal all neighbours | `enter` / `space` on revealed cell | left-click revealed cell |
| flag all neighbours | `f` on revealed cell | right-click revealed cell |
| hint | `?` | — |
| menu | `esc` | — |
| quit | `q` | — |

flag cycles: hidden → flagged → question → hidden.

## leaderboard

times are recorded for beginner, intermediate, and expert. games where hints were used are not ranked.

## hints

press `?` during a game to request a hint. the solver will highlight cells that can be deduced safe or as mines from the current board state. using a hint disqualifies the run from the leaderboard.

boards are guaranteed to be solvable without guessing — a solvable layout is pre-generated in the background as you hover before your first click.
