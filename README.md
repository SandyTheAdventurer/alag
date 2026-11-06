# ALAG

**A** simple **L**ogging **A**nd **G**raphing tool, for researchers.

## Why

Existing experiment trackers ask for more than a researcher who wants to log a run and look at the results should have to give:

- **Weights & Biases** requires an account and other setup.
- **MLflow** needs a tracking server that is heavy for this job, and comparing runs in it feels awkward.
- **TensorBoard** has visualizations that leave a lot to be desired, and makes comparing multiple runs hard.

ALAG aims to do two things well, logging and graphing, without the overhead. Comparing multiple runs is a core feature.

## Install

```sh
pip install alag
```

This gives you the `ALAG` class for your training code and the `alag` command for viewing results.

## Usage

Log from your training code:

```python
from alag import ALAG

logger = ALAG(group="my-group", experiment="my-experiment", config={"lr": 3e-4})

logger.log({"train/loss": 0.42}, step=100)
logger.log_image("confusion-matrix", "confusion.png", step=100)  # from a file
logger.log_image("loss-curve", fig, step=100)                    # from a matplotlib figure
```

Then view your experiments:

```sh
alag                # serves ./logs at http://localhost:6100
alag path/to/logs --port 8000
```

If an experiment with that name already exists in the group, ALAG creates a new one with a number on the end, such as `my-experiment-2`, so a re-run never mixes with earlier results.

To view experiments on a remote machine, run `alag` there and forward the port with `ssh -L 6100:localhost:6100 my-server`.

## Speed

`logger.log` is built to stay out of your training loop. It only puts the values on an in-memory queue and returns. A background thread formats them and writes to disk about once a second.

You can pass GPU tensors straight to `logger.log`. The background thread reads them, so your training loop doesn't wait for the GPU the way it does with `loss.item()`.

`logger.log_image` does its work immediately, because matplotlib isn't thread-safe. Log images occasionally, not on every step.

Queued values are written when your program exits, including after an exception. If the process is killed outright, you can lose about the last second of values.

## What it logs

- **Scalars**, such as loss and accuracy.
- **Images**, such as custom graphs you make yourself.

## Storage

Experiments are organized into groups. Each experiment is a plain folder inside its group's folder. There is no database or account.

```
logs/
└── my-group/
    └── my-experiment/
        ├── config.json    # the experiment's settings
        ├── metrics.csv    # every scalar, appended as it is logged
        └── images/        # logged images
```

`metrics.csv` has one row per logged value, so an experiment can add new metrics at any point and metrics logged at different intervals don't leave empty cells:

```csv
step,time,metric,value
100,1762444800.1,train/loss,0.42
100,1762444800.1,train/acc,0.81
200,1762444812.6,train/loss,0.37
200,1762444812.7,val/loss,0.45
```

## Viewing experiments

The `alag` command is a small compiled Rust program that generates an HTML page and serves it on localhost. The page updates automatically while your experiments are training.

A panel on the left lists your groups and the experiments inside them. Select experiments in the panel to compare them.
