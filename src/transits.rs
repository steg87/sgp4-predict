use std::ops::Range;

use chrono::Duration;
use chrono::{DateTime, Utc};

use crate::Error;
use crate::Observation;
use crate::Predictor;
use crate::observe::Observer;
use crate::time::IntervalRange;
use crate::units::{self, SI};

const MAX_STEP: Duration = Duration::minutes(5);

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
    prev_sample: Option<TransitSample>,
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
            prev_sample: None,
        }
    }

    /// Takes a new observation and determines if a transit has been entered by comparing the new
    /// observation state with the previous.
    ///
    /// On entering a transit it will calculate the roots (start, end) of the transit and return it.
    fn detect_transit(&self, sample: &TransitSample) -> Option<Transit> {
        // Determine if state transition indicates that a new transit has been found
        match &self.prev_sample {
            // Previous example exists, check if we have transitioned into a transit
            Some(prev) => {
                match (&prev.state, &sample.state) {
                    (TransitIterState::OutsideTransit, TransitIterState::InsideTransit) => {
                        // Transitioned into a transit, find the roots of the transit and return it
                        Some(self.calculate_transit(sample)) // TODO
                    }
                    _ => None, // No other state transitions of interest
                }
            }
            None => {
                match sample.state {
                    TransitIterState::InsideTransit
                        if sample.observation.elevation == self.min_elevation =>
                    {
                        // This is an edge case where the first observation is the start of a
                        // transit, i.e. the start of the first transit is exactly concurrent with
                        // the start of iter interval.
                        Some(self.calculate_transit(sample)) // TODO
                    }
                    _ => None,
                }
            }
        }
    }

    fn calculate_transit(&self, sample: &TransitSample) -> Transit {
        Transit {
            start: sample.time,
            end: sample.time,
        } // TODO
    }

    fn step_size(&self, sample: &TransitSample) -> Duration {
        // If we are in a transit already then choose max step size
        if matches!(sample.state, TransitIterState::InsideTransit) {
            return MAX_STEP;
        }
        // Check if previous sample available to estimate elevation rate
        match &self.prev_sample {
            Some(prev) => {
                // Elevation rate available, use it to estimate step size
                let (t0, e0) = (prev.time, prev.observation.elevation.to_si());
                let (t1, e1) = (sample.time, sample.observation.elevation.to_si());
                let el_rate = (e1 - e0) / (t1 - t0).as_seconds_f64();
                if el_rate <= 0.0 {
                    // Elevation rate is decreasing we are far away from entering a transit
                    return MAX_STEP;
                }
                let step =
                    (self.min_elevation.to_si() - sample.observation.elevation.to_si()) / el_rate;
                // Limit step size to MAX_STEP
                std::cmp::min(Duration::nanoseconds((step * 1e9) as i64), MAX_STEP)
            }
            None => {
                // No previous sample to estimate elevation rate from, choose something conservative
                Duration::minutes(1)
            }
        }
    }
}

impl<'a, O: Observer> Iterator for TransitIter<'a, O> {
    type Item = Result<Transit, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.interval.contains(&self.next_time) {
            // Calculate observation at current time
            let t = self.next_time;
            let observation = match self.predictor.propagate(t) {
                Ok(teme_state) => teme_state.to_ecef(t).to_enu(self.observer).to_observation(),
                Err(e) => return Some(Err(e)),
            };
            let state = if observation.elevation >= self.min_elevation {
                TransitIterState::InsideTransit
            } else {
                TransitIterState::OutsideTransit
            };
            let sample = TransitSample {
                time: t,
                observation,
                state,
            };

            // Detect transit, if any. Calculate step size based on result.
            let result = self.detect_transit(&sample);
            // Calculate next step size
            self.next_time += self.step_size(&sample);
            // Update previous sample
            self.prev_sample.replace(sample);
            // If transit found then return it, otherwise continue
            if let Some(transit) = result {
                return Some(Ok(transit));
            }
        }
        None
    }
}

struct TransitSample {
    time: DateTime<Utc>,
    observation: Observation,
    state: TransitIterState,
}

#[derive(Debug, Clone)]
enum TransitIterState {
    InsideTransit,
    OutsideTransit,
}
