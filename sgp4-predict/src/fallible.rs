//! Error-handling adapters for the crate's fallible iterators.
//!
//! Every detection and sampling iterator yields [`Result<T>`](crate::Result),
//! because the failures that reach `next()` fall into two classes with opposite
//! correct handling:
//!
//! - **Local** — a single event failed to refine ([`Error::Roots`],
//!   [`Error::Detect`]). The scan has already advanced, so the next event is
//!   unaffected and skipping is right. Use [`FallibleIter::log_errors`],
//!   [`FallibleIter::skip_errors`] or [`FallibleIter::on_error`].
//! - **Sticky** — [`Error::Sgp4`] is degenerate propagation state, deterministic
//!   in the elements. A decayed TLE fails at *every* sample, and skipping turns
//!   that into an empty iterator, indistinguishable from "no passes this week".
//!   Use [`FallibleIter::tolerate_errors`] or [`FallibleIter::until_error`],
//!   which stop and retain the error.
//!
//! [`Error::Roots`]: crate::Error::Roots
//! [`Error::Detect`]: crate::Error::Detect
//! [`Error::Sgp4`]: crate::Error::Sgp4

use std::{fmt, iter::FusedIterator};

use crate::{Error, Result};

/// Drops an error. The handler behind [`FallibleIter::skip_errors`].
fn drop_error(_: Error) {}

/// Logs an error at `warn`. The handler behind [`FallibleIter::log_errors`].
fn log_error(e: Error) {
    tracing::warn!(error = %e, "skipping failed iteration");
}

/// Ergonomic error handling for any iterator of [`Result<T>`](crate::Result).
///
/// Implemented for every such iterator, so all of this crate's iterators have
/// these methods. It is in the [prelude](crate::prelude).
///
/// ```no_run
/// use chrono::{Duration, Utc};
/// use sgp4_predict::prelude::*;
///
/// # fn main() -> Result<()> {
/// # let tle = Tle::new("SENTINEL-2C",
/// #     "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990",
/// #     "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740");
/// let predictor = Predictor::from_tle(&tle)?;
/// let glasgow = GroundObserver::new(Degrees(55.86), Degrees(-4.25), 40.0);
/// let start = Utc::now();
///
/// for transit in predictor
///     .transits_iter(&glasgow, start..start + Duration::days(1), Degrees(5.0))
///     .log_errors()
/// {
///     println!("AoS {}  LoS {}", transit.start, transit.end);
/// }
/// # Ok(())
/// # }
/// ```
pub trait FallibleIter<T>: Iterator<Item = Result<T>> + Sized {
    /// Passes each error to `f` and continues with the next item.
    ///
    /// ```
    /// use sgp4_predict::{Error, FallibleIter, Result};
    ///
    /// let items: Vec<Result<i32>> = vec![Ok(1), Err(Error::Custom("bad".into())), Ok(2)];
    /// let mut seen = Vec::new();
    /// let values: Vec<i32> = items.into_iter().on_error(|e| seen.push(e)).collect();
    ///
    /// assert_eq!(values, [1, 2]);
    /// assert_eq!(seen.len(), 1);
    /// ```
    fn on_error<F: FnMut(Error)>(self, f: F) -> OnError<Self, F> {
        OnError {
            iter: self,
            handler: f,
        }
    }

    /// Discards errors silently, yielding only the successful items.
    ///
    /// [`Iterator::flatten`] does the same thing on an iterator of `Result`,
    /// but does not say so at the call site.
    ///
    /// ```
    /// use sgp4_predict::{Error, FallibleIter, Result};
    ///
    /// let items: Vec<Result<i32>> = vec![Ok(1), Err(Error::Custom("bad".into())), Ok(2)];
    /// assert_eq!(items.into_iter().skip_errors().collect::<Vec<_>>(), [1, 2]);
    /// ```
    fn skip_errors(self) -> OnError<Self, fn(Error)> {
        self.on_error(drop_error)
    }

    /// Logs each error at `warn` via [`tracing`] and continues.
    ///
    /// ```
    /// use sgp4_predict::{Error, FallibleIter, Result};
    ///
    /// let items: Vec<Result<i32>> = vec![Ok(1), Err(Error::Custom("bad".into())), Ok(2)];
    /// assert_eq!(items.into_iter().log_errors().collect::<Vec<_>>(), [1, 2]);
    /// ```
    fn log_errors(self) -> OnError<Self, fn(Error)> {
        self.on_error(log_error)
    }

    /// Skips up to `max_consecutive` errors in a row, then stops and retains the
    /// error that broke the limit. Any successful item resets the run.
    ///
    /// An unbroken run of failures is evidence the propagation itself is dead,
    /// rather than one event having failed to refine.
    ///
    /// Iterate `&mut` if you want to query [`Tolerate::error`] afterwards.
    ///
    /// ```
    /// use sgp4_predict::{Error, FallibleIter, Result};
    ///
    /// let err = || Err(Error::Custom("bad".into()));
    /// let items: Vec<Result<i32>> = vec![Ok(1), err(), Ok(2), err(), err(), Ok(3)];
    ///
    /// let mut it = items.into_iter().tolerate_errors(1);
    /// let values: Vec<i32> = it.by_ref().collect();
    ///
    /// assert_eq!(values, [1, 2], "the isolated error is tolerated, the pair is not");
    /// assert!(it.error().is_some());
    /// ```
    fn tolerate_errors(self, max_consecutive: usize) -> Tolerate<Self> {
        Tolerate {
            iter: self,
            max_consecutive,
            run: 0,
            error: None,
        }
    }

    /// Stops at the first error and retains it. `tolerate_errors(0)`.
    ///
    /// ```
    /// use sgp4_predict::{Error, FallibleIter, Result};
    ///
    /// let items: Vec<Result<i32>> = vec![Ok(1), Err(Error::Custom("bad".into())), Ok(2)];
    ///
    /// let mut it = items.into_iter().until_error();
    /// assert_eq!(it.by_ref().collect::<Vec<_>>(), [1]);
    /// assert!(matches!(it.into_error(), Some(Error::Custom(_))));
    /// ```
    fn until_error(self) -> Tolerate<Self> {
        self.tolerate_errors(0)
    }
}

impl<I, T> FallibleIter<T> for I where I: Iterator<Item = Result<T>> {}

/// Yields the successful items of a fallible iterator, passing each error to a
/// handler. Created by [`FallibleIter::on_error`], [`FallibleIter::skip_errors`]
/// and [`FallibleIter::log_errors`].
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone)]
pub struct OnError<I, F> {
    iter: I,
    handler: F,
}

// Hand-written: a derive would bound `Debug` on `F`, and the general case of
// `on_error` is a closure, which is never `Debug`.
impl<I: fmt::Debug, F> fmt::Debug for OnError<I, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OnError").field("iter", &self.iter).finish()
    }
}

impl<I, F, T> Iterator for OnError<I, F>
where
    I: Iterator<Item = Result<T>>,
    F: FnMut(Error),
{
    type Item = T;

    fn next(&mut self) -> Option<T> {
        loop {
            match self.iter.next()? {
                Ok(v) => return Some(v),
                Err(e) => (self.handler)(e),
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.iter.size_hint().1)
    }
}

impl<I, F, T> FusedIterator for OnError<I, F>
where
    I: FusedIterator<Item = Result<T>>,
    F: FnMut(Error),
{
}

/// Yields the successful items of a fallible iterator until too many errors
/// occur in a row, then stops and retains the error. Created by
/// [`FallibleIter::tolerate_errors`] and [`FallibleIter::until_error`].
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Debug, Clone)]
pub struct Tolerate<I> {
    iter: I,
    max_consecutive: usize,
    run: usize,
    error: Option<Error>,
}

impl<I> Tolerate<I> {
    /// The error that stopped iteration, if it has stopped.
    #[must_use]
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }

    /// Consumes the adapter, returning the error that stopped iteration.
    #[must_use]
    pub fn into_error(self) -> Option<Error> {
        self.error
    }
}

impl<I, T> Iterator for Tolerate<I>
where
    I: Iterator<Item = Result<T>>,
{
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if self.error.is_some() {
            return None;
        }
        loop {
            match self.iter.next()? {
                Ok(v) => {
                    self.run = 0;
                    return Some(v);
                }
                Err(e) => {
                    self.run += 1;
                    if self.run > self.max_consecutive {
                        tracing::warn!(error = %e, run = self.run, "stopping iteration");
                        self.error = Some(e);
                        return None;
                    }
                    tracing::debug!(error = %e, run = self.run, "tolerating failed iteration");
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.iter.size_hint().1)
    }
}

impl<I, T> FusedIterator for Tolerate<I> where I: FusedIterator<Item = Result<T>> {}
