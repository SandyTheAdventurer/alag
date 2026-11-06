import csv
import json
import os
import time

import pytest

from alag import ALAG


def rows(logger):
    with open(os.path.join(logger.path, "metrics.csv"), newline="") as f:
        return list(csv.DictReader(f))


def test_layout_and_config(tmp_path):
    with ALAG("g", "e", config={"lr": 0.1}, root=tmp_path) as logger:
        logger.log({"loss": 1.5}, step=3)
    assert json.load(open(tmp_path / "g" / "e" / "config.json")) == {"lr": 0.1}
    assert rows(logger) == [
        {"step": "3", "time": rows(logger)[0]["time"], "metric": "loss", "value": "1.5"}
    ]


def test_rerun_gets_a_new_folder(tmp_path):
    with ALAG("g", "e", root=tmp_path) as first, ALAG("g", "e", root=tmp_path) as second:
        pass
    assert first.experiment == "e"
    assert second.experiment == "e-2"


def test_new_metric_and_awkward_name(tmp_path):
    with ALAG("g", "e", root=tmp_path) as logger:
        logger.log({"a": 1})
        logger.log({"b,with \"quotes\"": 2})
    assert [r["metric"] for r in rows(logger)] == ["a", 'b,with "quotes"']
    assert [r["step"] for r in rows(logger)] == ["0", "1"]


def test_nan_is_kept_and_non_scalars_are_skipped(tmp_path):
    with ALAG("g", "e", root=tmp_path) as logger:
        logger.log({"bad": float("nan"), "text": "hello", "ok": 1})
    assert [(r["metric"], r["value"]) for r in rows(logger)] == [
        ("bad", "nan"),
        ("ok", "1.0"),
    ]


def test_background_thread_writes_without_close(tmp_path):
    logger = ALAG("g", "e", root=tmp_path)
    logger.log({"a": 1})
    time.sleep(1.5)
    assert len(rows(logger)) == 1
    logger.close()


def test_torch_tensor(tmp_path):
    torch = pytest.importorskip("torch")
    with ALAG("g", "e", root=tmp_path) as logger:
        loss = (torch.ones(1, requires_grad=True) * 2).sum()
        logger.log({"loss": loss})
    assert rows(logger)[0]["value"] == "2.0"


def test_image_from_path_and_figure(tmp_path):
    plt = pytest.importorskip("matplotlib.pyplot")
    source = tmp_path / "in.png"
    source.write_bytes(b"fake")
    fig = plt.figure()
    with ALAG("g", "e", root=tmp_path) as logger:
        logger.log_image("val/a", source, step=5)
        logger.log_image("b", fig, step=7)
    images = tmp_path / "g" / "e" / "images"
    assert (images / "val" / "a" / "5.png").read_bytes() == b"fake"
    assert (images / "b" / "7.png").stat().st_size > 0


@pytest.mark.parametrize("group", ["", "..", "a/b"])
def test_bad_names(tmp_path, group):
    with pytest.raises(ValueError):
        ALAG(group, "e", root=tmp_path)
