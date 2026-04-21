# TurboPlot Changelog

## [X.X.X] - XXXX-XX-XX


## [1.2.0] - 2026-04-21

- Added support for Tektronix WFM files
- Added support for 2D Numpy arrays
- Added `--npy-layout` (`auto`, `columns`, `rows`) to disambiguate 2D Numpy files: column-wise dumps are now correctly detected and transposed so the rest of the app treats them like row-wise traces, fixing rendering of oscilloscope `(time, voltage)` arrays (#11).
- `--frames` is now available for every format and its behavior no longer depends on the NPY layout: it selects trace rows for row-wise arrays and trace columns for column-wise arrays.
- Added a safety cap on the total number of split views to keep the UI responsive.

## [1.1.0] - 2026-02-02

- Added traces' names and paths on the toolbar.
- Rainbow color scale is now the default.
- Added a file and settings dialog on startup when CLI is not used.
- Fixed measurement tool display when there are multiple views.
- Fixed zooming issue when traces were not synced on X shift.

## [1.0.0] - 2025-10-15

- Added ability to view multiple traces and synchronize the views.
- Added ability to filter traces on opening.
- Added support for CSV file format.
- Upgraded egui and eframe crates versions.

## [0.2.2] - 2025-09-25

- Faster implementation of CPU renderer.

## [0.2.1] - 2025-09-23

- Fixed reverted Y-axis orientation.

## [0.2.0] - 2025-09-23

- Antialiased lines rendering mode for high zoom values.
- Increased zoom limit.
