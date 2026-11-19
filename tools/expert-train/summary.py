"""Summarise an expert-data folder: the teacher's scores and how well the hand-made shortlist and the lead covered
them. The regret of a pick set is the teacher's best score less the best score among the set (what a look-ahead that
tried only that set gives up), "what the tree does" always in the set.

    python tools/expert-train/summary.py <data dir> [<data dir> ...]
"""

import glob
import sys

import numpy as np


def load(dirs):
    parts = {k: [] for k in ("features", "scores", "shortlist", "lead", "meta")}
    for d in dirs:
        for g in sorted(glob.glob(f"{d}/g[0-9]*[0-9]")):
            for k in parts:
                parts[k].append(np.load(f"{g}/{k}.npy"))
    return {k: np.concatenate(v) for k, v in parts.items()}


def regret(scores, picks):
    best = scores.max(axis=1)
    got = np.where(picks, scores, -np.inf).max(axis=1)
    return best - got


def main():
    d = load(sys.argv[1:])
    s, short, lead = d["scores"], d["shortlist"].astype(bool), d["lead"].astype(bool)
    n, a = s.shape
    print(f"{n} samples, {d['features'].shape[1]} features, {a} actions, NaN scores {np.isnan(s).sum()}")
    spread = s.max(axis=1) - s.min(axis=1)
    print(f"score spread per sample: median {np.median(spread):.1f}, mean {spread.mean():.1f}, "
          f"flat (< 1) {np.mean(spread < 1) * 100:.0f} %")
    names = open(f"{sys.argv[1]}/actions.txt").read().split("\n")
    best = s.argmax(axis=1)
    counts = np.bincount(best, minlength=a)
    order = np.argsort(-counts)
    print("teacher's best action, most often first:")
    print("  " + ", ".join(f"{names[i]} {counts[i] / n * 100:.1f}%" for i in order[:15]))
    for name, picks in (("shortlist", short), ("lead", lead)):
        r = regret(s, picks)
        top = picks[np.arange(n), best]
        print(f"{name}: picks {picks.sum(axis=1).mean():.1f}, has the best {top.mean() * 100:.0f} %, "
              f"regret mean {r.mean():.2f}, > 5 points {np.mean(r > 5) * 100:.1f} %")


if __name__ == "__main__":
    main()
