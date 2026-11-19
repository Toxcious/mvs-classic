# Expert network training

The Expert bot's network (`docs/expert-bot-plan.md`, "X5") picks which actions its look-ahead tries. These scripts
train it on the look-ahead's own scores; Python 3 with NumPy and PyTorch (a CUDA build if there is a GPU).

1. Data: `expert-data` (`crates/mvs-sim-trace`) plays bot games on the dev store and writes the teacher's samples
   to a folder under `.scratch/` (never committed): `cargo run --profile trace -p mvs-sim-trace --bin expert-data --
   --out <dir> --games 500 --stages trainingroomsmall,treehouse,classic3platform_large,batcave`.
2. `python tools/expert-train/summary.py <dir>` prints what the data holds and how the hand-made shortlist does.
3. `python tools/expert-train/train.py --data <dir> --out <file>` trains and writes the weights file.
4. A second round on the states the network-led bot reaches: `expert-data --lead net --net <file> --out <dir2>`,
   then train on both folders (`--data <dir> --data <dir2>`).
5. `bot-arena --level-a 4 --level-b 4 --lead-a net --lead-b x4 --net <file>` measures it; the weights the game uses
   are `crates/mvs-sim/src/ai/expert/expert.net`.

Run the long steps at below-normal priority (the Rust tools lower their own).
