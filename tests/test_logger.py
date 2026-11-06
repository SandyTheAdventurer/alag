import csv
import json
import os
import time

import pytest

from alag import ALAG, logger as logger_module


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


def test_tags_and_note(tmp_path):
    with ALAG("g", "e", root=tmp_path, tags=["a"], note="first") as logger:
        logger.tag("b", "a")
        logger.note("second")
    meta = json.load(open(tmp_path / "g" / "e" / "meta.json"))
    assert meta == {"tags": ["a", "b"], "note": "second"}


def test_flags_are_shared_by_the_group_without_duplicates(tmp_path):
    with ALAG("g", "e", root=tmp_path) as first, ALAG("g", "f", root=tmp_path) as second:
        first.flag("val/loss", below=0.5, by_step=1000)
        second.flag("val/loss", below=0.5, by_step=1000)
        second.flag("acc", above=0.9, by_step=2000)
    assert json.load(open(tmp_path / "g" / "flags.json")) == [
        {"metric": "val/loss", "direction": "below", "value": 0.5, "step": 1000},
        {"metric": "acc", "direction": "above", "value": 0.9, "step": 2000},
    ]


def test_flag_needs_one_direction(tmp_path):
    with ALAG("g", "e", root=tmp_path) as logger:
        with pytest.raises(ValueError):
            logger.flag("a", by_step=1)
        with pytest.raises(ValueError):
            logger.flag("a", by_step=1, below=1, above=2)


def test_sections_and_pins_are_shared_by_the_group(tmp_path):
    with ALAG("g", "e", root=tmp_path) as first, ALAG("g", "f", root=tmp_path) as second:
        first.section("validation", "val/loss", "bleu")
        first.pin("val/loss")
        second.pin("val/loss", "confusion-matrix")
        second.section("other", "bleu")
    assert json.load(open(tmp_path / "g" / "layout.json")) == {
        "sections": {"val/loss": "validation", "bleu": "other"},
        "pinned": ["val/loss", "confusion-matrix"],
    }


def test_system_metrics_are_off_by_default(tmp_path, monkeypatch):
    monkeypatch.setattr(logger_module, "_SYSTEM_INTERVAL", 0.01)
    with ALAG("g", "e", root=tmp_path) as logger:
        time.sleep(0.1)
    assert logger._system_thread is None
    assert rows(logger) == []


def test_system_metrics_use_the_latest_step(tmp_path, monkeypatch):
    pytest.importorskip("psutil")
    monkeypatch.setattr(logger_module, "_SYSTEM_INTERVAL", 0.01)
    monkeypatch.setattr(
        logger_module, "_read_gpus", lambda: {"system/gpu0_util_percent": 50.0}
    )
    with ALAG("g", "e", root=tmp_path, system_metrics=True) as logger:
        logger.log({"loss": 1}, step=40)
        time.sleep(0.2)
    system = [r for r in rows(logger) if r["metric"].startswith("system/")]
    assert {r["metric"] for r in system} == {
        "system/cpu_percent",
        "system/ram_used_gb",
        "system/process_ram_gb",
        "system/gpu0_util_percent",
    }
    assert {r["step"] for r in system} == {"40"}


def test_system_metrics_keep_going_without_a_gpu(tmp_path, monkeypatch):
    pytest.importorskip("psutil")
    calls = []
    monkeypatch.setattr(logger_module, "_SYSTEM_INTERVAL", 0.01)
    monkeypatch.setattr(logger_module, "_read_gpus", lambda: calls.append(1))
    with ALAG("g", "e", root=tmp_path, system_metrics=True) as logger:
        time.sleep(0.2)
    assert len(calls) == 1  # no nvidia-smi, so it isn't tried again
    assert any(r["metric"] == "system/cpu_percent" for r in rows(logger))


def test_missing_psutil_fails_early_and_leaves_no_folder(tmp_path, monkeypatch):
    monkeypatch.setitem(__import__("sys").modules, "psutil", None)
    with pytest.raises(ImportError, match="alag\\[system\\]"):
        ALAG("g", "e", root=tmp_path, system_metrics=True)
    assert not (tmp_path / "g").exists()


def test_gpu_output_is_parsed_and_unavailable_fields_are_skipped():
    assert logger_module._parse_gpus("40, 2048, 61, [N/A]\n0, 512, 35, 12.5\n") == {
        "system/gpu0_util_percent": 40.0,
        "system/gpu0_memory_gb": 2.0,
        "system/gpu0_temperature_c": 61.0,
        "system/gpu1_util_percent": 0.0,
        "system/gpu1_memory_gb": 0.5,
        "system/gpu1_temperature_c": 35.0,
        "system/gpu1_power_w": 12.5,
    }
