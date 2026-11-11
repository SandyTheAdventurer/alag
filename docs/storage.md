# Storage

Experiments are organized into groups. Each experiment is a plain folder inside its group's folder. There is no database or account, so you can copy, back up, delete or read runs with any tool.

```
logs/
├── .trash/                # runs moved to trash from the viewer
└── my-group/
    ├── flags.json         # targets shared by the group
    ├── layout.json        # chart sections and pins shared by the group
    └── my-experiment/
        ├── config.json    # the experiment's settings
        ├── metrics.csv    # every scalar, appended as it is logged
        ├── meta.json      # tags and note
        └── images/        # logged images, as <name>/<step>.png
```

`metrics.csv` has one row per logged value, so an experiment can add new metrics at any point and metrics logged at different intervals don't leave empty cells:

```csv
step,time,metric,value
100,1762444800.1,train/loss,0.42
100,1762444800.1,train/acc,0.81
200,1762444812.6,train/loss,0.37
200,1762444812.7,val/loss,0.45
```

`time` is Unix time in seconds. To load a run in pandas and get one column per metric:

```python
import pandas as pd

df = pd.read_csv("logs/my-group/my-experiment/metrics.csv")
wide = df.pivot_table(index="step", columns="metric", values="value")
```
