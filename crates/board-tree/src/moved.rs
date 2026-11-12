//! Following a path under a moved prefix (docs/plugins/calling/commands.md,
//! Renamed plugins): a renamed plugin moves its records from one prefix to
//! another, and a reference written under either prefix still resolves
//! while a board holds them under the other.

use std::collections::VecDeque;

use board_model::{Location, Moved, swapped};

use crate::address::Pointers;
use crate::repositories::Repositories;
use crate::resolve::{ResolveError, Trail};
use crate::symlink::Chains;
use crate::tree::Tree;

impl Repositories {
    /// Follows `moved` when a resolution names nothing, in place of none.
    pub fn moving(&mut self, moved: Vec<Moved>) {
        self.moved = moved;
    }

    /// The moved prefixes this run follows.
    pub fn moved(&self) -> &[Moved] {
        &self.moved
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
        mut remaining: VecDeque<String>,
        from_link: usize,
        trail: &mut Trail,
        (pointers, chains): (&mut Pointers, &mut Chains<T::Directory>),
    ) -> Result<Location, ResolveError> {
        // The swap is computed before the walk, and is `None` without a
        // copy unless `remaining` begins with a moved prefix, so a board
        // with no moved prefixes, or a path under neither, walks as before
        // with nothing copied or kept.
        let other = (trail.moved && from_link == 0)
            .then(|| swapped(&self.moved, remaining.make_contiguous()))
            .flatten();
        let Some(other) = other else {
            return self.walk(tree, remaining, from_link, trail, (pointers, chains));
        };
        let (link, hops) = (trail.link.clone(), trail.pointers.len());
        let missed = match self.walk(tree, remaining, 0, trail, (&mut *pointers, &mut *chains)) {
            Err(missed @ ResolveError::NotFound { .. }) => missed,
            resolved => return resolved,
        };
        trail.link = link;
        trail.pointers.truncate(hops);
        match self.walk(tree, other.into(), 0, trail, (pointers, chains)) {
            Err(ResolveError::NotFound { .. }) => Err(missed),
            resolved => resolved,
        }
    }
}
