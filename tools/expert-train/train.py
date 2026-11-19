"""Train the Expert bot's network (docs/expert-bot-plan.md, "X5") on the teacher's samples `expert-data` writes, and
export its weights in the format `mvs-sim`'s `ai/expert/net.rs` reads.

    python tools/expert-train/train.py --data <dir> [--data <dir> ...] --out <weights file> [--epochs n] [--tau t]

The net ranks the action list as the look-ahead does: a listwise loss, the cross-entropy between the softmax of the
teacher's scores over `--tau` points and the softmax of the net's outputs. A rank is all the bot uses (it tries the
net's best five), and the scores' scale differs by orders between a quiet moment and a knockout, which a regression
would chase; a sample whose scores are all near equal teaches nothing and weighs less. The validation split is by
game. It prints, on the validation games, how often the net's top five (with "what the tree does") hold the
teacher's best action and the regret (the teacher's best score less the best in the set), beside the hand-made
shortlist's.
"""

import argparse
import glob
import struct
import time

import numpy as np
import torch
from torch import nn

TOP = 5


def load(dirs):
    parts = {k: [] for k in ("features", "scores", "shortlist", "meta")}
    for n, d in enumerate(dirs):
        for g in sorted(glob.glob(f"{d}/g[0-9]*[0-9]")):
            for k in parts:
                v = np.load(f"{g}/{k}.npy")
                if k == "meta":
                    v = v.copy()
                    v[:, 0] += n * 1_000_000  # games of each folder apart
                parts[k].append(v)
    return {k: np.concatenate(v) for k, v in parts.items()}


def picks_of(out, top=TOP):
    """The net's set: "what the tree does" (action 0) and its best `top` others."""
    o = out.clone()
    o[:, 0] = -float("inf")
    idx = o.topk(top, dim=1).indices
    m = torch.zeros_like(out, dtype=torch.bool)
    m.scatter_(1, idx, True)
    m[:, 0] = True
    return m


def regret(scores, picks):
    best = scores.max(dim=1).values
    got = torch.where(picks, scores, torch.full_like(scores, -float("inf"))).max(dim=1).values
    return best - got


def net_of(inputs, outputs, width, depth):
    layers, n = [], inputs
    for _ in range(depth):
        layers += [nn.Linear(n, width), nn.ReLU()]
        n = width
    layers.append(nn.Linear(n, outputs))
    return nn.Sequential(*layers)


def export(model, mean, std, path):
    """The weights file: the normalisation folded into the first layer, each layer input-major."""
    linears = [m for m in model if isinstance(m, nn.Linear)]
    blobs = []
    for i, l in enumerate(linears):
        w = l.weight.detach().double().cpu().numpy()  # (out, in)
        b = l.bias.detach().double().cpu().numpy()
        if i == 0:
            w = w / std[None, :]
            b = b - (w * mean[None, :]).sum(axis=1)
        blobs.append((w.T.astype("<f4"), b.astype("<f4")))
    with open(path, "wb") as f:
        f.write(b"MVSX")
        f.write(struct.pack("<IIII", 1, linears[0].in_features, linears[-1].out_features, len(linears)))
        for w, b in blobs:
            f.write(struct.pack("<II", w.shape[0], w.shape[1]))
            f.write(np.ascontiguousarray(w).tobytes())
            f.write(b.tobytes())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", action="append", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--epochs", type=int, default=12)
    ap.add_argument("--tau", type=float, default=2.0)
    ap.add_argument("--width", type=int, default=256)
    ap.add_argument("--depth", type=int, default=3)
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    torch.manual_seed(a.seed)
    dev = "cuda" if torch.cuda.is_available() else "cpu"
    d = load(a.data)
    x = d["features"].astype(np.float32)
    s = np.nan_to_num(d["scores"].astype(np.float32), nan=-1e4)
    games = d["meta"][:, 0]
    rng = np.random.default_rng(a.seed)
    ug = np.unique(games)
    val_games = set(rng.choice(ug, size=max(1, len(ug) // 10), replace=False).tolist())
    val = np.array([g in val_games for g in games])
    mean = x[~val].mean(axis=0).astype(np.float64)
    std = np.maximum(x[~val].std(axis=0), 1e-2).astype(np.float64)
    xt = torch.tensor((x - mean) / std, dtype=torch.float32, device=dev)
    st = torch.tensor(s, device=dev)
    spread = st.max(dim=1).values - st.min(dim=1).values
    weight = (spread / 5.0).clamp(0.05, 1.0)
    target = torch.softmax(st / a.tau, dim=1)
    tr_i = torch.tensor(np.nonzero(~val)[0], device=dev)
    va_i = torch.tensor(np.nonzero(val)[0], device=dev)
    print(f"{len(x)} samples ({len(tr_i)} train, {len(va_i)} validation, {len(ug)} games), {x.shape[1]} inputs, "
          f"{s.shape[1]} actions, on {dev}")
    model = net_of(x.shape[1], s.shape[1], a.width, a.depth).to(dev)
    opt = torch.optim.AdamW(model.parameters(), lr=1e-3, weight_decay=1e-4)
    steps_per = (len(tr_i) + 1023) // 1024
    sched = torch.optim.lr_scheduler.OneCycleLR(opt, max_lr=2e-3, total_steps=a.epochs * steps_per)
    short = torch.tensor(d["shortlist"].astype(bool), device=dev)
    sv = st[va_i]
    r_short = regret(sv, short[va_i])
    best = sv.argmax(dim=1)
    print(f"shortlist on validation: has the best {short[va_i][torch.arange(len(va_i)), best].float().mean():.3f}, "
          f"regret mean {r_short.mean():.3f}, > 5 points {(r_short > 5).float().mean():.3f}")
    t0 = time.time()
    for ep in range(a.epochs):
        model.train()
        perm = tr_i[torch.randperm(len(tr_i), device=dev)]
        total = 0.0
        for k in range(0, len(perm), 1024):
            b = perm[k:k + 1024]
            logp = torch.log_softmax(model(xt[b]), dim=1)
            loss = (-(target[b] * logp).sum(dim=1) * weight[b]).sum() / weight[b].sum()
            opt.zero_grad()
            loss.backward()
            opt.step()
            sched.step()
            total += loss.item() * len(b)
        if ep % 5 == 4 or ep == a.epochs - 1:
            model.eval()
            with torch.no_grad():
                out = model(xt[va_i])
                p = picks_of(out)
                r = regret(sv, p)
                has = p[torch.arange(len(va_i)), best].float().mean()
            print(f"epoch {ep + 1:>3}: loss {total / len(perm):.4f}; validation: net top {TOP} has the best "
                  f"{has:.3f}, regret mean {r.mean():.3f}, > 5 points {(r > 5).float().mean():.3f} "
                  f"({time.time() - t0:.0f} s)")
    export(model.cpu(), mean, std, a.out)
    print(f"written {a.out}")


if __name__ == "__main__":
    main()
