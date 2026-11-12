//! Following a path under a moved prefix (docs/plugins/calling/commands.md,
//! Renamed plugins): a renamed plugin moves its records from one prefix to
//! another, and a reference written under either prefix still resolves
//! while a board holds them under the other.

use std::cell::OnceCell;
use std::collections::VecDeque;
use std::fmt;

use board_model::{Location, Moved, swapped};

use crate::address::Pointers;
use crate::repositories::Repositories;
use crate::resolve::{ResolveError, Trail};
use crate::symlink::Chains;
use crate::tree::Tree;

/// The moved prefixes of one run, read from `read` the first time a
/// resolution names nothing, so a run whose paths all resolve never reads
/// them.
#[derive(Default)]
pub(crate) struct Moving {
    read: Option<Box<dyn Fn() -> Vec<Moved> + Send>>,
    moved: OnceCell<Vec<Moved>>,
}

impl fmt::Debug for Moving {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Moving")
            .field("moved", &self.moved.get())
            .finish_non_exhaustive()
    }
}

impl Repositories {
    /// Reads the moved prefixes from `read` when a resolution first names
    /// nothing, in place of none.
    pub fn moving(&mut self, read: impl Fn() -> Vec<Moved> + Send + 'static) {
        self.moving = Moving {
            read: Some(Box::new(read)),
            moved: OnceCell::new(),
        };
    }

    /// The moved prefixes this run follows.
    pub fn moved(&self) -> &[Moved] {
        let moving = &self.moving;
        moving
            .moved
            .get_or_init(|| moving.read.as_ref().map(|read| read()).unwrap_or_default())
    }

    /// Resolves `remaining` from the root of `tree` as
    /// [`Repositories::walk`] does. When that names nothing, `trail` allows
    /// moved prefixes and `remaining` begins with one, it resolves
    /// `remaining` under the other prefix; when that names nothing too, the
    /// first miss stands. Each board's root is tried, the root board's and
    /// each mounted one's. A path a link's target gave is left as written:
    /// a soft limit, since no link names a moved prefix yet; following one
    /// would need the swap inside the walk's restart at a link.
    pub(crate) fn follow<T: Tree>(
        &self,
        tree: &mut T,
        remaining: VecDeque<String>,
        from_link: usize,
        trail: &mut Trail,
        (pointers, chains): (&mut Pointers, &mut Chains<T::Directory>),
    ) -> Result<Location, ResolveError> {
        if !trail.moved || from_link > 0 {
            return self.walk(tree, remaining, from_link, trail, (pointers, chains));
        }
        let (link, hops, kept) = (trail.link.clone(), trail.pointers.len(), remaining.clone());
        let missed = match self.walk(tree, remaining, 0, trail, (&mut *pointers, &mut *chains)) {
            Err(missed @ ResolveError::NotFound { .. }) => missed,
            resolved => return resolved,
        };
        let Some(other) = swapped(self.moved(), Vec::from(kept).as_slice()) else {
            return Err(missed);
        };
        trail.link = link;
        trail.pointers.truncate(hops);
        match self.walk(tree, other.into(), 0, trail, (pointers, chains)) {
            Err(ResolveError::NotFound { .. }) => Err(missed),
            resolved => resolved,
        }
    }
}
