# Viewing

```sh
alag                          # serves ./logs at http://localhost:6100
alag path/to/logs --port 8000
```

`alag` is a small compiled Rust program that serves the viewer on localhost only. If port 6100 is taken, it tries the next ones. The page updates automatically while your experiments are training.

To view experiments on a remote machine, run `alag` there and forward the port:

```sh
ssh -L 6100:localhost:6100 my-server
```

## Comparing runs

A panel on the left lists your groups and the experiments inside them. Select experiments to compare them: each metric gets one chart, with a line for every selected experiment. Runs that are still logging are marked **live**. The filter box searches names, tags and notes.

Above the charts:

- **Smoothing** applies an exponential moving average. The raw line stays faintly behind it.
- **X axis** switches between step and time since the run started.
- **Log Y** uses a logarithmic y-axis.
- **Mean ± std over seeds** merges runs that differ only by seed (see below).

Drag across a chart to zoom in. Double-click or press **Reset zoom** to zoom out. Hover to read every run's value at that point.

## Tabs

- **Charts**: one chart per metric, grouped into sections.
- **Images**: each image name with a step slider, side by side for every selected run.
- **Table**: the latest value of every metric for every selected run.
- **Hyperparameters**: a row for each run and a column for each setting in its `config`. Settings that differ come first, in bold. Click a column to sort by it, or tick **Only settings that differ** to hide the rest.
- **Notes**: edit each run's tags and note, or move the run to trash.

## Sections and pins

Charts are grouped into sections by the part of the metric name before the first slash, so `val/loss` goes under `val` and `system/cpu_percent` goes under `system`.

To choose a section yourself, use **Section** on a chart or `logger.section`. **Pin** on a chart or image, or `logger.pin`, moves it to the top of its tab.

Sections and pins apply to the whole group, so they stay in place when you compare its experiments.

## Mean and std over seeds

Tick **Mean ± std over seeds** to merge selected experiments into one line with a shaded band. Experiments merge when they are in the same group and their configs differ only by a key named `seed`. Experiments with no `seed` in their config stay separate.

The band is one sample standard deviation at each step. Smoothing is applied to each seed before merging.

## Flags

A flag sets a target, such as "`val/loss` should go below 0.5 by step 1,000". Set one with `logger.flag`, or with **Flag** on a chart, where clicking the chart fills in the values.

The flag is drawn on the chart, and each experiment in the group is listed as reached, missed or not yet. Verdicts use the line as plotted, so they follow your smoothing and seed settings.

## Removing runs

In the **Notes** tab, click **Move to trash** under a run, then click again to confirm. The folder moves to `logs/.trash/<group>/` and disappears from the viewer. To bring a run back, move its folder back into the group. To free the disk space, delete `logs/.trash`.

A run that logged something in the last 15 seconds may still be training, so it can't be moved until it stops.
