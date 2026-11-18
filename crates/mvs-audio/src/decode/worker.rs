//! The decode worker: a thread of the game's side that decodes the media of a scope's lazy events when one is first
//! posted, so the game thread does not wait on Vorbis.

use std::sync::Arc;
use std::sync::mpsc;

use mvs_data::sound::Id;

use super::Pcm;
use super::cache::MediaCache;

/// A medium to decode: its id and its entry's path in the `sound` pack.
struct Job {
    id: Id,
    path: Box<str>,
}

/// A medium decoded, or why not.
pub(crate) struct Done {
    pub id: Id,
    pub pcm: Result<Arc<Pcm>, String>,
}

/// The thread is not joined: let go, it ends once it has nowhere to send an answer (after at most the medium it is
/// decoding and one more).
pub(crate) struct Worker {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
}

impl Worker {
    /// Starts the thread over the store's `sound` pack, its media decoded through `media`.
    pub(crate) fn start(store: Arc<mvs_store::Store>, media: Arc<MediaCache>) -> Worker {
        let (jobs, rx) = mpsc::channel::<Job>();
        let (tx, done) = mpsc::channel();
        std::thread::Builder::new()
            .name("mvs-audio-decode".to_owned())
            .spawn(move || {
                let pack = mvs_data::sound::sound_pack(&store).map_err(|e| e.to_string());
                while let Ok(j) = rx.recv() {
                    let pcm = match &pack {
                        // (a panic in the decoder is a bad medium: answered as an error, the thread goes on)
                        Ok(p) => {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| media.medium(p, j.id, &j.path)))
                                .unwrap_or_else(|_| {
                                    Err(super::DecodeError::Vorbis(format!("medium {}", j.id), "panic".into()))
                                })
                                .map(|g| g.pcm)
                                .map_err(|e| e.to_string())
                        }
                        Err(e) => Err(e.clone()),
                    };
                    if tx.send(Done { id: j.id, pcm }).is_err() {
                        return;
                    }
                }
            })
            .expect("audio: the decode worker's thread");
        Worker { jobs, done }
    }

    pub(crate) fn submit(&self, id: Id, path: &str) {
        let _ = self.jobs.send(Job { id, path: path.into() });
    }

    /// The media decoded since the last call.
    pub(crate) fn finished(&self) -> impl Iterator<Item = Done> + '_ {
        self.done.try_iter()
    }
}
