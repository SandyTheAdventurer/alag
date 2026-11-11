"""Writes made-up training runs for the docs' demo and screenshot.

Usage: python scripts/demo_logs.py LOGDIR
"""

import math
import random
import sys

from alag import ALAG

STEPS = 3000


def run(root, name, lr, seed, tags=(), note=None):
    rng = random.Random(seed)
    config = {"model": "resnet18", "dataset": "cifar10", "lr": lr, "batch_size": 128, "seed": seed}
    with ALAG("cifar-resnet", name, config=config, root=root, tags=list(tags), note=note) as logger:
        speed = lr / 3e-4
        floor = 0.12 + 0.05 * abs(math.log10(lr / 1e-3)) + rng.uniform(0, 0.02)
        for step in range(0, STEPS + 1, 10):
            progress = 1 - math.exp(-step * speed / 900)
            loss = floor + (2.3 - floor) * (1 - progress)
            values = {
                "train/loss": loss * (1 + rng.gauss(0, 0.06)),
                "train/lr": lr * 0.5 * (1 + math.cos(math.pi * step / STEPS)),
            }
            if step % 250 == 0:
                values["val/loss"] = loss * 1.15 + rng.gauss(0, 0.02)
                values["val/acc"] = min(0.97, 0.1 + 0.85 * progress) - floor / 3 + rng.gauss(0, 0.005)
            logger.log(values, step=step)
        logger.flag("val/loss", below=0.5, by_step=2000)
        logger.section("training", "train/loss", "train/lr")
        logger.section("validation", "val/loss", "val/acc")
        logger.pin("val/acc")


def main():
    root = sys.argv[1]
    for seed in (1, 2, 3):
        run(root, f"lr-3e-4-seed{seed}", 3e-4, seed, tags=["baseline"])
        run(root, f"lr-1e-3-seed{seed}", 1e-3, seed + 10,
            note="Higher learning rate: faster early on, similar final accuracy")


if __name__ == "__main__":
    main()
