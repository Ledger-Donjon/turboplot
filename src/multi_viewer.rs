use crate::{sync_features::SyncFeatures, tiling::Tiling, viewer::Viewer};
use egui::{Rect, pos2};
use std::sync::{Arc, Condvar, Mutex};

/// Split window space to display multiple traces using multiple [`Viewer`]. When enabled,
/// synchronizes the camera of the different viewers.
pub struct MultiViewer<'a> {
    /// Displayed viewers, first at window top, last at bottom.
    viewers: Vec<Viewer<'a>>,
    /// Selects which camera features should be synchronized.
    sync: SyncFeatures,
}

impl<'a> MultiViewer<'a> {
    pub fn new(
        ctx: &egui::Context,
        shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
        traces: &'a [Vec<f32>],
    ) -> Self {
        Self {
            viewers: traces
                .iter()
                .enumerate()
                .map(|(i, t)| Viewer::new(i as u32, ctx, shared_tiling.clone(), t))
                .collect(),
            sync: SyncFeatures::new(),
        }
    }

    /// Copy settings from viewer number `index` to others.
    fn sync(&mut self, index: usize) {
        let source_camera = *self.viewers[index].get_camera();
        for viewer in self
            .viewers
            .iter_mut()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, viewer)| viewer)
        {
            let mut camera = *viewer.get_camera();
            if self.sync.shift_x {
                camera.shift.x = source_camera.shift.x;
            }
            if self.sync.shift_y {
                camera.shift.y = source_camera.shift.y;
            }
            if self.sync.scale_x {
                camera.scale.x = source_camera.scale.x;
            }
            if self.sync.scale_y {
                camera.scale.y = source_camera.scale.y;
            }
            viewer.set_camera(camera);
        }
    }

    /// Updates and paints all the viewers.
    fn update(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let size = ui.available_size();
        let n = self.viewers.len();
        let h = size.y / n as f32;

        // Calculate the viewport for each viewer.
        // We need viewports for both update and paint.
        let viewports: Vec<_> = (0..self.viewers.len())
            .map(|i| Rect::from_min_max(pos2(0.0, i as f32 * h), pos2(size.x, (i + 1) as f32 * h)))
            .collect();

        // Call update of each viewer, don't do the painting yet because we might change viewer
        // settings afterwards for synchronization.
        let status: Vec<_> = self
            .viewers
            .iter_mut()
            .zip(viewports.iter())
            .map(|(viewer, viewport)| viewer.update(ctx, ui, *viewport))
            .collect();

        // If some viewer changes and synchronization is performed, we use this flag to prevent
        // other viewers to request tiles while dragging or zooming is not finished yet.
        let mut allow_tile_requests_for_all = true;

        if self.sync.any() {
            // Check if a viewer has changing camera settings
            if let Some((sync_index, status)) = status
                .iter()
                .enumerate()
                .find(|(_, status)| status.dragging_x || status.dragging_y || status.zooming)
            {
                // dragging_x is not used here, it is ok to request for tiles when dragging along
                // X-axis. Since the scale does not change, only missing tiles on the left or right
                // will be requested, which is not heavy.
                allow_tile_requests_for_all &= !status.zooming && !status.dragging_y;
                // Viewer number sync_index has changed, we must copy settings to others.
                self.sync(sync_index);
            }
        }

        // Paint all toolbars first: if we detect that synchronization is turned on we have to
        // perform sync before painting waveforms.
        let mut sync_index = None;
        for (index, (viewer, viewport)) in self.viewers.iter_mut().zip(viewports.iter()).enumerate()
        {
            let prev_sync = self.sync;
            viewer.paint_toolbar(
                ctx,
                if n > 1 { Some(&mut self.sync) } else { None },
                *viewport,
            );

            if (!prev_sync & self.sync).any() {
                // One option has been enabled.
                sync_index = Some(index);
            }
        }

        if let Some(sync_index) = sync_index {
            self.sync(sync_index)
        }

        // Now that all viewers have been updated and synchronized, we can paint them.
        for ((viewer, viewport), status) in self
            .viewers
            .iter_mut()
            .zip(viewports.iter())
            .zip(status.iter())
        {
            let allow_tile_requests = !status.zooming && !status.dragging_y;
            viewer.paint_waveform(
                ctx,
                ui,
                *viewport,
                allow_tile_requests && allow_tile_requests_for_all,
            );
        }
    }
}

impl<'a> eframe::App for MultiViewer<'a> {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().outer_margin(0.0))
            .show(ctx, |ui| {
                self.update(ctx, ui);
            });
    }
}
