use std::ops::Range;

use chrono::Duration;
use chrono::{DateTime, Utc};

use crate::Error;
use crate::Observation;
use crate::Predictor;
use crate::observe::Observer;
use crate::time::IntervalRange;
use crate::units;

pub struct Transit {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl IntervalRange for Transit {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

pub struct TransitIter<'a, O: Observer> {
    predictor: Predictor,
    observer: &'a O,
    interval: Range<DateTime<Utc>>,
    min_elevation: units::Angle,
    next_time: DateTime<Utc>,
    state: Option<TransitIterState>,
}

impl<'a, O: Observer> TransitIter<'a, O> {
    pub fn new(
        predictor: Predictor,
        observer: &'a O,
        interval: impl IntervalRange,
        min_elevation: units::Angle,
    ) -> Self {
        Self {
            predictor,
            observer,
            interval: interval.start()..interval.end(),
            min_elevation,
            next_time: interval.start(),
            state: None,
        }
    }

    /// Takes a new observation and determines if a transit has been entered by comparing the new
    /// observation state with the previous.
    ///
    /// On entering a transit it will calculate the roots of the transit and return it.
    fn detect_transit(&mut self, t: DateTime<Utc>, observation: &Observation) -> Option<Transit> {
        let state = if observation.elevation >= self.min_elevation {
            TransitIterState::InsideTransit
        } else {
            TransitIterState::OutsideTransit
        };
        let result = match (&self.state, &state) {
            (Some(TransitIterState::OutsideTransit), TransitIterState::InsideTransit) => {
                // Transitioned into a transit, find the roots of the transit and return it
                Some(Transit { start: t, end: t }) // TODO
            }
            (None, TransitIterState::InsideTransit) => {
                // This is an edge case where the first observation is in a transit. It is likely
                // that this transit is of no interest because it started before the iter interval
                // but there is a chance the transit and iter interval started concurrently, so it
                // must be checked for strict correctness.
                Some(Transit { start: t, end: t }) // TODO
            }
            _ => None, // All other state transitions are irrelevant
        };
        // Update the state before returning result
        self.state = Some(state);
        result
    }

    fn step_size(&self, _observation: &Observation) -> Duration {
        Duration::minutes(1)
    }
}

impl<'a, O: Observer> Iterator for TransitIter<'a, O> {
    type Item = Result<Transit, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.interval.contains(&self.next_time) {
            let t = self.next_time;
            let observation = match self.predictor.propagate(t) {
                Ok(teme_state) => teme_state.to_ecef(t).to_enu(self.observer).to_observation(),
                Err(e) => return Some(Err(e)),
            };
            match self.detect_transit(t, &observation) {
                Some(transit) => return Some(Ok(transit)),
                None => self.next_time += self.step_size(&observation),
            }
        }
        None
    }
}

enum TransitIterState {
    InsideTransit,
    OutsideTransit,
}
