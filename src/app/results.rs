use super::*;
use crate::thumbnail;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResultFilter {
    #[default]
    All,
    Uploaded,
    Exists,
    Failed,
    Uncertain,
    Pending,
}

impl ResultFilter {
    pub fn matches(self, outcome: &ItemOutcome) -> bool {
        match self {
            Self::All => true,
            Self::Uploaded => matches!(outcome, ItemOutcome::Uploaded),
            Self::Exists => matches!(outcome, ItemOutcome::Exists),
            Self::Failed => matches!(outcome, ItemOutcome::Failed(_)),
            Self::Uncertain => matches!(outcome, ItemOutcome::Uncertain(_)),
            Self::Pending => matches!(outcome, ItemOutcome::Pending),
        }
    }
}

#[derive(Default)]
pub(super) struct ResultCounts {
    pub(super) uploaded: usize,
    pub(super) exists: usize,
    pub(super) failed: usize,
    pub(super) uncertain: usize,
    pub(super) pending: usize,
}

impl ResultCounts {
    pub fn needs_processing(&self) -> usize {
        self.failed + self.uncertain + self.pending
    }
}

impl BackupTask {
    pub fn counts(&self) -> ResultCounts {
        let mut counts = ResultCounts::default();
        for outcome in self.outcomes.values() {
            match outcome {
                ItemOutcome::Uploaded => counts.uploaded += 1,
                ItemOutcome::Exists => counts.exists += 1,
                ItemOutcome::Failed(_) => counts.failed += 1,
                ItemOutcome::Uncertain(_) => counts.uncertain += 1,
                ItemOutcome::Pending => counts.pending += 1,
            }
        }
        counts
    }
}

impl UploadApp {
    pub(super) fn filter_results(&mut self, filter: ResultFilter, cx: &mut Context<Self>) {
        self.backup.filter = filter;
        self.backup.page = 0;
        self.request_details_thumbnails(cx);
        cx.notify();
    }

    pub(super) fn request_thumbnail(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.thumbnails.contains_key(&path) {
            return;
        }
        self.thumbnail_sequence = self.thumbnail_sequence.wrapping_add(1);
        let sequence = self.thumbnail_sequence;
        self.thumbnails
            .insert(path.clone(), Thumbnail::Loading(sequence));
        cx.spawn(async move |this, cx| {
            let source = path.clone();
            let result = cx.background_spawn(async move { thumbnail::load(&source) }).await;
            let _ = this.update(cx, |app, cx| {
                if matches!(app.thumbnails.get(&path), Some(Thumbnail::Loading(current)) if *current == sequence) {
                    app.thumbnails.insert(path, match result {
                        Ok(image) => Thumbnail::Ready(image),
                        Err(_) => Thumbnail::Unavailable,
                    });
                    cx.notify();
                }
            });
        }).detach();
    }

    pub(super) fn request_details_thumbnails(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<_> = self
            .queue
            .files
            .iter()
            .filter(|file| {
                self.backup
                    .outcomes
                    .get(&file.path)
                    .is_some_and(|outcome| self.backup.filter.matches(outcome))
            })
            .skip(self.backup.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .map(|file| file.path.clone())
            .collect();
        self.thumbnails.retain(|path, _| paths.contains(path));
        for path in paths {
            self.request_thumbnail(path, cx);
        }
    }

    pub(super) fn set_result_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.backup.page = page;
        self.request_details_thumbnails(cx);
        cx.notify();
    }
}
