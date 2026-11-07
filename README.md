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

logger.tag("baseline", "ablation")
logger.note("Loss spikes after the LR warmup ends")
logger.flag("val/loss", below=0.5, by_step=1000)
logger.section("validation", "val/loss", "bleu")
logger.pin("val/loss")
```

`ALAG` also takes `tags=[...]`, `note="..."` and `system_metrics=True` directly.

Then view your experiments:

```sh
alag                # serves ./logs at http://localhost:6100
alag path/to/logs --port 8000
alag --export report.html   # one HTML file to send to someone
```

If an experiment with that name already exists in the group, ALAG creates a new one with a number on the end, such as `my-experiment-2`, so a re-run never mixes with earlier results.

To view experiments on a remote machine, run `alag` there and forward the port with `ssh -L 6100:localhost:6100 my-server`.

## Speed

`logger.log` is built to stay out of your training loop. It only puts the values on an in-memory queue and returns. A background thread formats them and writes to disk about once a second.

You can pass GPU tensors straight to `logger.log`. The background thread reads them, so your training loop doesn't wait for the GPU the way it does with `loss.item()`.

On an RTX 3050 laptop GPU, a `logger.log` call with a GPU tensor took about 15 µs. A 6 ms training step was no slower with logging than without it, with or without system metrics. Any difference was smaller than the variation between runs.

`logger.log_image` does its work immediately, because matplotlib isn't thread-safe. Log images occasionally, not on every step.

Queued values are written when your program exits, including after an exception. If the process is killed outright, you can lose about the last second of values.

## What it logs

- **Scalars**, such as loss and accuracy.
- **Images**, such as custom graphs you make yourself.
- **Hyperparameters**, from the `config` you pass to `ALAG`.
- **System metrics**, if you turn them on.

### System metrics

These are off by default. To turn them on, install the extra and pass `system_metrics=True`:

```sh
pip install 'alag[system]'
```

```python
logger = ALAG(group="my-group", experiment="my-experiment", system_metrics=True)
```

Every 10 seconds, a background thread logs CPU use, system RAM and your process's RAM under `system/`. If `nvidia-smi` is available, it also logs utilization, memory, temperature and power for each GPU. GPU memory is the total in use on that card, including other programs.

Each sample uses your latest training step. Before your first `logger.log`, or during a long step, several samples share a step, so set **X axis** to **Time** to read these charts.

## Storage

Experiments are organized into groups. Each experiment is a plain folder inside its group's folder. There is no database or account.

```
logs/
└── my-group/
    ├── flags.json         # targets shared by the group
    ├── layout.json        # chart sections and pins shared by the group
    └── my-experiment/
        ├── config.json    # the experiment's settings
        ├── metrics.csv    # every scalar, appended as it is logged
        ├── meta.json      # tags and note
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

A panel on the left lists your groups and the experiments inside them. Select experiments in the panel to compare them: each metric gets one chart, with a line for every selected experiment.

### Hyperparameters

The **Hyperparameters** tab has a row for each selected experiment and a column for each setting in its `config`. Settings that differ come first, in bold. Click a column to sort by it, or tick **Only settings that differ** to hide the rest.

### Sections and pins

Charts are grouped into sections by the part of the metric name before the first slash, so `val/loss` goes under `val` and `system/cpu_percent` goes under `system`.

To choose a section yourself, use **Section** on a chart or `logger.section`. **Pin** on a chart or image, or `logger.pin`, moves it to the top of its tab.

Sections and pins apply to the whole group, so they stay in place when you compare its experiments.

### Mean and std over seeds

Tick **Mean ± std over seeds** to merge selected experiments into one line with a shaded band. Experiments merge when they are in the same group and their configs differ only by a key named `seed`. Experiments with no `seed` in their config stay separate.

The band is one sample standard deviation at each step. Smoothing is applied to each seed before merging.

### Flags

A flag sets a target, such as "`val/loss` should go below 0.5 by step 1,000". Set one with `logger.flag`, or with **Flag** on a chart, where clicking the chart fills in the values.

The flag is drawn on the chart, and each experiment in the group is listed as reached, missed or not yet. Verdicts use the line as plotted, so they follow your smoothing and seed settings.

### Notes and tags

Add these in the **Notes** tab or from Python. Tags appear in the left panel, and the filter box searches names, tags and notes.

### Exporting charts

Each chart has **SVG**, **PNG**, **PDF**, **CSV** and **LaTeX** buttons. Exports use a white background and match the current view, including every selected experiment, smoothing, zoom, log scale and flags.

**LaTeX** gives you a `pgfplots` figure to copy and paste. The data is included, so there's no file to upload, but long runs are thinned to keep it small enough for pdflatex. Your document needs:

```latex
\usepackage{pgfplots}
\pgfplotsset{compat=1.18}
```

### Sharing a report

`alag --export report.html` writes one HTML file that opens in any browser, with no server or install. It opens on the most recently updated group, with its experiments selected.

You can still select experiments, zoom, smooth, merge seeds and export charts. It's a read-only snapshot, so it doesn't update, and notes, tags, flags, sections and pins can't be changed.

The file contains everything in the log folder, including every group, config, note and image. To share only part of it, export a folder with just those groups. Runs with many images can make the file large.

## License

MIT
