# Sharing

## Exporting charts

Each chart has **SVG**, **PNG**, **PDF**, **CSV** and **LaTeX** buttons. Exports use a white background and match the current view, including every selected experiment, smoothing, zoom, log scale and flags.

**LaTeX** gives you a `pgfplots` figure to copy and paste. The data is included, so there's no file to upload, but long runs are thinned to keep it small enough for pdflatex. Your document needs:

```latex
\usepackage{pgfplots}
\pgfplotsset{compat=1.18}
```

## Sharing a report

```sh
alag --export report.html
alag path/to/logs --export report.html
```

This writes one HTML file that opens in any browser, with no server or install. It opens on the most recently updated group, with its experiments selected. The [live demo](demo.html) on this site is one of these files.

You can still select experiments, zoom, smooth, merge seeds and export charts. It's a read-only snapshot, so it doesn't update, and notes, tags, flags, sections and pins can't be changed.

!!! warning "The file holds everything"
    The file contains everything in the log folder, including every group, config, note and image. To share only part of it, export a folder with just those groups. Runs with many images can make the file large.
