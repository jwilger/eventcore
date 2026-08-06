//! Experimental runtime-coupled Event Modeling support.
//!
//! This module is intentionally feature-gated. Enable `experimental-modeling`
//! to construct and execute modeled commands, then add
//! `experimental-model-check` in test targets to run the static checker.

use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use eventcore_types::{
    CommandError, CommandLogic, CommandStreams, Event, NewEvents, Projector, StreamId,
    StreamPosition, StreamResolver,
};
use thiserror::Error;

#[cfg(feature = "experimental-model-check")]
use std::collections::{BTreeMap, BTreeSet};

/// A semantic value that identifies an EventCore stream.
///
/// Domain newtypes derive this trait rather than exposing conversion methods for
/// every possible role. The modeled command derive uses it when declaring the
/// command's consistency boundary.
pub trait StreamIdentity {
    /// Returns the underlying EventCore stream identifier.
    fn as_stream_id(&self) -> &StreamId;
}

impl StreamIdentity for StreamId {
    fn as_stream_id(&self) -> &StreamId {
        self
    }
}

/// Marker for a modeled field occurrence.
pub trait ModelField {
    /// The Rust value carried by the field.
    type Value;
}

/// Typestate used by generated modeled builders before a field is assigned.
#[doc(hidden)]
pub struct Unset;

/// Typestate used by generated modeled builders after a field is assigned.
#[doc(hidden)]
pub struct Set<T>(T);

impl<T> Set<T> {
    /// Wraps an assigned builder value.
    #[doc(hidden)]
    #[must_use]
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Extracts an assigned builder value.
    #[doc(hidden)]
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

/// A value that has been produced for one exact modeled field occurrence.
#[repr(transparent)]
pub struct FieldValue<F: ModelField> {
    value: F::Value,
    marker: PhantomData<F>,
}

impl<F: ModelField> FieldValue<F> {
    /// Constructs a field value for generated modeled builders.
    ///
    /// This is public solely so derive and mapping macro expansions in an
    /// application crate can construct values. Application code should use
    /// generated builders and mappings instead.
    #[doc(hidden)]
    #[must_use]
    pub fn from_value(value: F::Value) -> Self {
        Self {
            value,
            marker: PhantomData,
        }
    }

    /// Returns the wrapped value.
    #[must_use]
    pub fn into_inner(self) -> F::Value {
        self.value
    }
}

/// Converts a generated mapping result into the exact field expected by a
/// modeled builder.
pub trait IntoFieldValue<F: ModelField> {
    /// Performs the conversion.
    fn into_field_value(self) -> FieldValue<F>;
}

impl<F: ModelField> IntoFieldValue<F> for FieldValue<F> {
    fn into_field_value(self) -> FieldValue<F> {
        self
    }
}

/// A value assembled through a modeled builder.
#[repr(transparent)]
pub struct Modeled<T>(T);

impl<T> Modeled<T> {
    /// Constructs a modeled value for generated builders.
    #[doc(hidden)]
    #[must_use]
    pub fn from_built(value: T) -> Self {
        Self(value)
    }

    /// Consumes the wrapper at an explicit application boundary.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> AsRef<T> for Modeled<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> Clone for Modeled<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Modeled<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Modeled").field(&self.0).finish()
    }
}

/// Marker implemented by a modeled input type.
pub trait ModelInput {}

/// Marker implemented by a modeled command type.
pub trait ModelCommand {}

/// Marker implemented by a modeled event type.
pub trait ModelEvent {}

/// A modeled state with an executable, modeled initial value.
pub trait ModelState: Send + Sized + 'static {
    /// Produces the initial state through its generated default/absence recipes.
    fn initial() -> Modeled<Self>;
}

impl<S: ModelState> Default for Modeled<S> {
    fn default() -> Self {
        S::initial()
    }
}

/// Marker implemented by a modeled projection effect.
pub trait ModelEffect: Send + Sized + 'static {}

/// Purely applies an effect to a specific modeled read model.
///
/// This is separate from [`ModelEffect`] so `#[derive(ModelEffect)]` stays a
/// useful marker derive. Effects that are persisted by
/// [`InMemoryProjectionSink`] additionally implement this contract.
pub trait ModelEffectApplication<R: ModelReadModel>: ModelEffect {
    /// Applies the effect to the previous modeled read model.
    fn apply_to(self, previous: Modeled<R>) -> Modeled<R>;
}

/// Marker implemented by a modeled read model.
pub trait ModelReadModel {}

/// Marker implemented by a modeled output value.
pub trait ModelOutput {}

/// Events produced by a modeled command.
pub struct ModeledEvents<E: Event> {
    events: Vec<Modeled<E>>,
}

impl<E: Event> ModeledEvents<E> {
    /// Produces one modeled event.
    #[must_use]
    pub fn one(event: Modeled<E>) -> Self {
        Self {
            events: vec![event],
        }
    }

    /// Produces no event for an explicit domain reason.
    #[must_use]
    pub fn none(_reason: &'static str) -> Self {
        Self { events: Vec::new() }
    }

    /// Appends another modeled event.
    pub fn push(&mut self, event: Modeled<E>) {
        self.events.push(event);
    }

    fn into_new_events(self) -> NewEvents<E> {
        self.events
            .into_iter()
            .map(Modeled::into_inner)
            .collect::<Vec<_>>()
            .into()
    }
}

/// Business logic for a command executing through the modeled lane.
pub trait ModelCommandLogic: CommandStreams + Send + Sync + Sized + 'static {
    /// Event type emitted by this command.
    type Event: Event + ModelEvent;

    /// Reconstructed modeled state.
    type State: ModelState;

    /// Evolves state from one historical event.
    fn evolve(&self, state: Modeled<Self::State>, event: &Self::Event) -> Modeled<Self::State>;

    /// Makes a decision using the reconstructed modeled state.
    fn decide(
        &self,
        state: Modeled<Self::State>,
    ) -> Result<ModeledEvents<Self::Event>, CommandError>;

    /// Discovers streams needed after modeled state reconstruction.
    fn discover_related_streams(&self, _state: &Modeled<Self::State>) -> Vec<StreamId> {
        Vec::new()
    }
}

/// A command that can enter the existing EventCore executor through the modeled
/// runtime lane.
#[repr(transparent)]
pub struct ModeledCommand<C>(C);

impl<C> ModeledCommand<C> {
    /// Constructs the wrapper from a generated modeled command builder.
    #[doc(hidden)]
    #[must_use]
    pub fn from_built(command: C) -> Self {
        Self(command)
    }
}

impl<C> AsRef<C> for ModeledCommand<C> {
    fn as_ref(&self) -> &C {
        &self.0
    }
}

impl<C: CommandStreams> CommandStreams for ModeledCommand<C> {
    fn stream_declarations(&self) -> eventcore_types::StreamDeclarations {
        self.0.stream_declarations()
    }
}

impl<C: ModelCommandLogic> StreamResolver<Modeled<C::State>> for ModeledCommand<C> {
    fn discover_related_streams(&self, state: &Modeled<C::State>) -> Vec<StreamId> {
        self.0.discover_related_streams(state)
    }
}

impl<C: ModelCommandLogic> CommandLogic for ModeledCommand<C> {
    type Event = C::Event;
    type State = Modeled<C::State>;

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        self.0.evolve(state, event)
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        self.0.decide(state).map(ModeledEvents::into_new_events)
    }

    fn stream_resolver(&self) -> Option<&(dyn StreamResolver<Self::State> + Sync)> {
        Some(self)
    }
}

/// The outcome of projecting one modeled event.
pub enum ProjectionAction<E: ModelEffect> {
    /// Persist a modeled effect.
    Apply(Modeled<E>),
    /// Intentionally ignore this event.
    Ignore(ModeledIgnore),
}

/// An explicit projection ignore decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeledIgnore {
    reason: &'static str,
}

impl ModeledIgnore {
    /// Records why an event is intentionally ignored.
    #[must_use]
    pub const fn new(reason: &'static str) -> Self {
        Self { reason }
    }

    /// Returns the recorded reason.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        self.reason
    }
}

/// A projection implemented through modeled effects.
pub trait ModelProjection: Send + 'static {
    /// Event type consumed by the projection.
    type Event: Event + ModelEvent;

    /// Effect emitted for an applied event.
    type Effect: ModelEffect + Send + 'static;

    /// Projection-domain error.
    type Error;

    /// Stable projection name.
    fn name(&self) -> &str;

    /// Produces a modeled effect or explicit ignore decision.
    fn project(
        &mut self,
        event: Self::Event,
        position: StreamPosition,
    ) -> Result<ProjectionAction<Self::Effect>, Self::Error>;
}

/// Imperative persistence boundary for modeled projection effects.
pub trait ProjectionSink<E: ModelEffect>: Send + 'static {
    /// Sink error.
    type Error;

    /// Persists an effect at its source stream position.
    fn apply(&mut self, effect: E, position: StreamPosition) -> Result<(), Self::Error>;
}

/// A typed in-memory sink for exercising modeled projections without an
/// imperative database adapter.
///
/// It is deliberately small and intended for tests, examples, and local
/// validation. Production sinks remain ordinary [`ProjectionSink`] adapters.
pub struct InMemoryProjectionSink<R: ModelReadModel> {
    state: Arc<Mutex<Option<Modeled<R>>>>,
}

impl<R: ModelReadModel> Clone for InMemoryProjectionSink<R> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl<R: ModelReadModel> InMemoryProjectionSink<R> {
    /// Creates a sink from a modeled initial read model.
    #[must_use]
    pub fn new(initial: Modeled<R>) -> Self {
        Self {
            state: Arc::new(Mutex::new(Some(initial))),
        }
    }

    /// Returns a clone of the current modeled read model.
    #[must_use]
    pub fn state(&self) -> Modeled<R>
    where
        R: Clone,
    {
        self.state
            .lock()
            .expect("in-memory projection sink mutex poisoned")
            .as_ref()
            .expect("in-memory projection sink always retains a state")
            .clone()
    }
}

impl<R, E> ProjectionSink<E> for InMemoryProjectionSink<R>
where
    R: ModelReadModel + Send + 'static,
    E: ModelEffectApplication<R>,
{
    type Error = std::convert::Infallible;

    fn apply(&mut self, effect: E, _position: StreamPosition) -> Result<(), Self::Error> {
        let mut state = self
            .state
            .lock()
            .expect("in-memory projection sink mutex poisoned");
        let previous = state
            .take()
            .expect("in-memory projection sink always retains a state");
        *state = Some(effect.apply_to(previous));
        Ok(())
    }
}

/// Error returned by a checked projection adapter.
#[derive(Debug, Error)]
pub enum CheckedProjectionError<P, S> {
    /// The pure projection failed.
    #[error("modeled projection failed")]
    Projection(P),
    /// The imperative sink failed.
    #[error("modeled projection sink failed")]
    Sink(S),
}

/// Adapter that lets a modeled projection use the existing projection runner.
pub struct CheckedProjector<P, S> {
    projection: P,
    sink: S,
}

/// Adapts a modeled projection and its sink into an EventCore [`Projector`].
#[must_use]
pub fn checked_projection<P, S>(projection: P, sink: S) -> CheckedProjector<P, S>
where
    P: ModelProjection,
    S: ProjectionSink<P::Effect>,
{
    CheckedProjector { projection, sink }
}

impl<P, S> CheckedProjector<P, S> {
    /// Consumes the adapter and returns its projection and sink.
    #[must_use]
    pub fn into_parts(self) -> (P, S) {
        (self.projection, self.sink)
    }
}

impl<P, S> Projector for CheckedProjector<P, S>
where
    P: ModelProjection,
    S: ProjectionSink<P::Effect>,
{
    type Event = P::Event;
    type Error = CheckedProjectionError<P::Error, S::Error>;
    type Context = ();

    fn apply(
        &mut self,
        event: Self::Event,
        position: StreamPosition,
        _context: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        match self
            .projection
            .project(event, position)
            .map_err(CheckedProjectionError::Projection)?
        {
            ProjectionAction::Apply(effect) => self
                .sink
                .apply(effect.into_inner(), position)
                .map_err(CheckedProjectionError::Sink),
            ProjectionAction::Ignore(_) => Ok(()),
        }
    }

    fn name(&self) -> &str {
        self.projection.name()
    }
}

/// Renders an output through a modeled read-model boundary.
pub trait ModelView {
    /// Source read model.
    type ReadModel: ModelReadModel;

    /// Rendered output.
    type Output: ModelOutput;

    /// Renders the output using modeled data.
    fn render(&self, model: &Modeled<Self::ReadModel>) -> Modeled<Self::Output>;
}

/// Options controlling an information-completeness check.
#[cfg(feature = "experimental-model-check")]
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    allow_all_assumptions: bool,
    allowed_assumptions: BTreeSet<&'static str>,
}

#[cfg(feature = "experimental-model-check")]
impl CheckOptions {
    /// Permits explicitly registered assumption boundaries.
    #[must_use]
    pub fn allow_assumptions(mut self) -> Self {
        self.allow_all_assumptions = true;
        self
    }

    /// Permits one named assumption boundary.
    #[must_use]
    pub fn allow_assumption(mut self, name: &'static str) -> Self {
        let _ = self.allowed_assumptions.insert(name);
        self
    }
}

/// Final status of a model check.
#[cfg(feature = "experimental-model-check")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// All required modeled fields have a complete executable provenance path.
    Verified,
    /// The model is complete only through an accepted explicit assumption.
    Assumed,
}

/// One deterministic checker diagnostic.
#[cfg(feature = "experimental-model-check")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDiagnostic {
    /// Stable diagnostic code.
    pub code: &'static str,
    /// Field or mapping that failed validation.
    pub subject: String,
    /// Human-readable explanation.
    pub message: String,
    /// Suggested next action.
    pub help: String,
    /// Provenance path observed while checking, when applicable.
    pub trace: Vec<String>,
    /// Source location captured from the modeled derive or mapping, when known.
    pub location: Option<String>,
}

#[cfg(feature = "experimental-model-check")]
impl std::fmt::Display for CheckDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: {}\n  {}\nhelp: {}",
            self.code, self.subject, self.message, self.help
        )?;
        if !self.trace.is_empty() {
            write!(formatter, "\ntrace: {}", self.trace.join(" -> "))?;
        }
        if let Some(location) = &self.location {
            write!(formatter, "\nlocation: {location}")?;
        }
        Ok(())
    }
}

/// Successful deterministic checker report.
#[cfg(feature = "experimental-model-check")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Verification status.
    pub status: CheckStatus,
    /// Non-fatal diagnostics sorted by code and subject.
    pub warnings: Vec<CheckDiagnostic>,
}

/// Failed deterministic checker report.
#[cfg(feature = "experimental-model-check")]
#[derive(Debug, Error)]
#[error("event model is incomplete")]
pub struct CheckError {
    /// Errors sorted by code and subject.
    pub diagnostics: Vec<CheckDiagnostic>,
}

/// Runs strict information-completeness checking for all linked modeled
/// descriptors.
#[cfg(feature = "experimental-model-check")]
pub fn check() -> Result<CheckReport, CheckError> {
    check_with(CheckOptions::default())
}

/// Runs information-completeness checking with explicit options.
#[cfg(feature = "experimental-model-check")]
pub fn check_with(options: CheckOptions) -> Result<CheckReport, CheckError> {
    check_references(inventory::iter::<Descriptor>.into_iter().collect(), options)
}

/// Checks an explicit descriptor set without using the linked-program registry.
///
/// This is useful for benchmark and checker-unit fixtures. Applications should
/// normally use [`check`] so registration continues to come from executable
/// modeled derives and mappings.
#[cfg(feature = "experimental-model-check")]
pub fn check_descriptors(
    descriptors: &[Descriptor],
    options: CheckOptions,
) -> Result<CheckReport, CheckError> {
    check_references(descriptors.iter().collect(), options)
}

#[cfg(feature = "experimental-model-check")]
fn check_references(
    mut descriptors: Vec<&Descriptor>,
    options: CheckOptions,
) -> Result<CheckReport, CheckError> {
    descriptors.sort_by_key(|descriptor| descriptor.stable_id());

    if descriptors.is_empty() {
        return Err(CheckError {
            diagnostics: vec![diagnostic(
                "ECM001",
                "registry",
                "no modeled descriptors were supplied for checking",
                "enable `experimental-model-check` and link the application model, or provide benchmark descriptors explicitly",
            )],
        });
    }

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut fields = BTreeMap::new();
    let mut mappings: BTreeMap<&str, Vec<&Descriptor>> = BTreeMap::new();
    let mut assumptions: BTreeMap<&str, Vec<&Descriptor>> = BTreeMap::new();
    let mut identifiers = BTreeSet::new();

    for descriptor in descriptors {
        if !identifiers.insert(descriptor.stable_id()) {
            errors.push(diagnostic_at(
                "ECM002",
                descriptor.stable_id(),
                "duplicate modeled descriptor registration",
                "give the component or mapping a unique name",
                descriptor.location(),
            ));
        }
        match descriptor.kind {
            DescriptorKind::Field { field, .. } => {
                let _ = fields.insert(field, descriptor);
            }
            DescriptorKind::Mapping { target, .. } => {
                mappings.entry(target).or_default().push(descriptor);
            }
            DescriptorKind::Assumption { target, .. } => {
                assumptions.entry(target).or_default().push(descriptor);
            }
        }
    }

    let graph = CheckerGraph {
        fields: &fields,
        mappings: &mappings,
        assumptions: &assumptions,
        options: &options,
    };
    let mut evaluation = CheckerEvaluation {
        errors,
        ..CheckerEvaluation::default()
    };
    for (field, descriptor) in &fields {
        if let DescriptorKind::Field { root, .. } = descriptor.kind
            && root
        {
            let _ = evaluation.memo.insert((*field).to_owned(), true);
            continue;
        }
        evaluation.visiting.clear();
        if !is_complete(field, &graph, &mut evaluation) {
            continue;
        }
    }

    let consumed_fields: BTreeSet<&str> = mappings
        .values()
        .flatten()
        .flat_map(|descriptor| match descriptor.kind {
            DescriptorKind::Mapping { sources, .. } => sources.to_vec(),
            _ => Vec::new(),
        })
        .collect();
    for (field, descriptor) in &fields {
        let DescriptorKind::Field { root, role, .. } = descriptor.kind else {
            continue;
        };
        if root && !consumed_fields.contains(field) {
            warnings.push(diagnostic(
                "ECM102",
                *field,
                "modeled origin is not consumed by any mapping",
                "remove the unused field or add an executable mapping that consumes it",
            ));
        }
        if role == "event" && !consumed_fields.contains(field) {
            warnings.push(diagnostic(
                "ECM103",
                *field,
                "modeled event field is not consumed downstream",
                "add a projection/view mapping or mark the event boundary intentionally opaque",
            ));
        }
    }

    let output_exists = fields.values().any(|descriptor| {
        matches!(
            descriptor.kind,
            DescriptorKind::Field { role: "output", .. }
        )
    });
    if !output_exists {
        warnings.push(diagnostic(
            "ECM101",
            "model",
            "the linked model has no modeled output",
            "add a ModelOutput/ModelView boundary when this model renders a view",
        ));
    }

    sort_diagnostics(&mut evaluation.errors);
    sort_diagnostics(&mut warnings);
    if evaluation.errors.is_empty() {
        Ok(CheckReport {
            status: if evaluation.used_assumption {
                CheckStatus::Assumed
            } else {
                CheckStatus::Verified
            },
            warnings,
        })
    } else {
        Err(CheckError {
            diagnostics: evaluation.errors,
        })
    }
}

#[cfg(feature = "experimental-model-check")]
struct CheckerGraph<'a> {
    fields: &'a BTreeMap<&'a str, &'a Descriptor>,
    mappings: &'a BTreeMap<&'a str, Vec<&'a Descriptor>>,
    assumptions: &'a BTreeMap<&'a str, Vec<&'a Descriptor>>,
    options: &'a CheckOptions,
}

#[cfg(feature = "experimental-model-check")]
#[derive(Default)]
struct CheckerEvaluation {
    used_assumption: bool,
    memo: BTreeMap<String, bool>,
    visiting: BTreeSet<String>,
    errors: Vec<CheckDiagnostic>,
    path: Vec<String>,
}

#[cfg(feature = "experimental-model-check")]
fn is_complete(field: &str, graph: &CheckerGraph<'_>, evaluation: &mut CheckerEvaluation) -> bool {
    let CheckerGraph {
        fields,
        mappings,
        assumptions,
        options,
    } = graph;
    if let Some(result) = evaluation.memo.get(field) {
        return *result;
    }
    if let Some(descriptor) = fields.get(field)
        && let DescriptorKind::Field { root, .. } = descriptor.kind
        && root
    {
        let _ = evaluation.memo.insert(field.to_owned(), true);
        return true;
    }
    if !evaluation.visiting.insert(field.to_owned()) {
        let mut trace = evaluation.path.clone();
        trace.push(field.to_owned());
        evaluation.errors.push(diagnostic_with_trace(
            "ECM006",
            field,
            "ordinary provenance cycle has no explicit temporal seed",
            "use `previous(...)` with a modeled default/absence seed",
            trace,
        ));
        let _ = evaluation.memo.insert(field.to_owned(), false);
        return false;
    }
    evaluation.path.push(field.to_owned());

    if let Some(boundaries) = assumptions.get(field) {
        let accepted = boundaries.iter().find_map(|boundary| {
            let DescriptorKind::Assumption { name, .. } = boundary.kind else {
                return None;
            };
            (options.allow_all_assumptions || options.allowed_assumptions.contains(name))
                .then_some(name)
        });
        if accepted.is_some() {
            evaluation.used_assumption = true;
            let _ = evaluation.visiting.remove(field);
            let _ = evaluation.memo.insert(field.to_owned(), true);
            let _ = evaluation.path.pop();
            return true;
        }
        for boundary in boundaries {
            let DescriptorKind::Assumption { name, .. } = boundary.kind else {
                continue;
            };
            evaluation.errors.push(diagnostic(
                "ECM008",
                name,
                format!("assumption boundary for `{field}` is not enabled"),
                format!("use `CheckOptions::default().allow_assumption(\"{name}\")` or replace it with an executable mapping"),
            ));
        }
        let _ = evaluation.visiting.remove(field);
        let _ = evaluation.memo.insert(field.to_owned(), false);
        let _ = evaluation.path.pop();
        return false;
    }

    let Some(recipes) = mappings.get(field) else {
        let location = fields
            .get(field)
            .map_or("<unknown>", |descriptor| descriptor.location());
        evaluation.errors.push(diagnostic_with_trace_at(
            "ECM003",
            field,
            "non-root modeled field has no executable producer",
            "add a mapping, default, absence, or explicit origin",
            evaluation.path.clone(),
            location,
        ));
        let _ = evaluation.visiting.remove(field);
        let _ = evaluation.memo.insert(field.to_owned(), false);
        let _ = evaluation.path.pop();
        return false;
    };

    let mut complete = true;
    for recipe in recipes {
        let DescriptorKind::Mapping {
            name,
            sources,
            temporal_sources,
            ..
        } = recipe.kind
        else {
            continue;
        };
        for (source, temporal) in sources.iter().zip(temporal_sources.iter()) {
            if !fields.contains_key(source) {
                let mut trace = evaluation.path.clone();
                trace.push((*source).to_owned());
                evaluation.errors.push(diagnostic_with_trace_at(
                    "ECM004",
                    name,
                    format!("mapping source `{source}` is not a modeled field"),
                    "derive a modeled component for the source owner or correct the mapping path",
                    trace,
                    recipe.location(),
                ));
                complete = false;
                continue;
            }
            if evaluation.visiting.contains(*source) && !temporal {
                evaluation.errors.push(diagnostic(
                    "ECM006",
                    name,
                    format!("mapping closes an ordinary cycle through `{source}`"),
                    "mark the carried state input as `previous(...)` and provide a default seed",
                ));
                complete = false;
                continue;
            }
            if evaluation.visiting.contains(*source) {
                if !has_non_temporal_seed(source, fields, mappings) {
                    evaluation.errors.push(diagnostic(
                        "ECM007",
                        name,
                        format!("temporal source `{source}` has no non-temporal seed"),
                        "add a default, absence, origin, or non-temporal mapping for the carried field",
                    ));
                    complete = false;
                }
                continue;
            }
            if !is_complete(source, graph, evaluation) {
                complete = false;
            }
        }
    }
    let _ = evaluation.visiting.remove(field);
    let _ = evaluation.memo.insert(field.to_owned(), complete);
    let _ = evaluation.path.pop();
    complete
}

#[cfg(feature = "experimental-model-check")]
fn has_non_temporal_seed(
    field: &str,
    fields: &BTreeMap<&str, &Descriptor>,
    mappings: &BTreeMap<&str, Vec<&Descriptor>>,
) -> bool {
    if let Some(descriptor) = fields.get(field)
        && let DescriptorKind::Field { root, .. } = descriptor.kind
        && root
    {
        return true;
    }

    mappings.get(field).is_some_and(|recipes| {
        recipes.iter().any(|recipe| {
            matches!(
                recipe.kind,
                DescriptorKind::Mapping {
                    temporal_sources,
                    ..
                } if temporal_sources.iter().all(|temporal| !temporal)
            )
        })
    })
}

#[cfg(feature = "experimental-model-check")]
fn diagnostic(
    code: &'static str,
    subject: impl Into<String>,
    message: impl Into<String>,
    help: impl Into<String>,
) -> CheckDiagnostic {
    CheckDiagnostic {
        code,
        subject: subject.into(),
        message: message.into(),
        help: help.into(),
        trace: Vec::new(),
        location: None,
    }
}

#[cfg(feature = "experimental-model-check")]
fn diagnostic_at(
    code: &'static str,
    subject: impl Into<String>,
    message: impl Into<String>,
    help: impl Into<String>,
    location: &'static str,
) -> CheckDiagnostic {
    CheckDiagnostic {
        code,
        subject: subject.into(),
        message: message.into(),
        help: help.into(),
        trace: Vec::new(),
        location: Some(location.to_owned()),
    }
}

#[cfg(feature = "experimental-model-check")]
fn diagnostic_with_trace(
    code: &'static str,
    subject: impl Into<String>,
    message: impl Into<String>,
    help: impl Into<String>,
    trace: Vec<String>,
) -> CheckDiagnostic {
    CheckDiagnostic {
        code,
        subject: subject.into(),
        message: message.into(),
        help: help.into(),
        trace,
        location: None,
    }
}

#[cfg(feature = "experimental-model-check")]
fn diagnostic_with_trace_at(
    code: &'static str,
    subject: impl Into<String>,
    message: impl Into<String>,
    help: impl Into<String>,
    trace: Vec<String>,
    location: &'static str,
) -> CheckDiagnostic {
    CheckDiagnostic {
        code,
        subject: subject.into(),
        message: message.into(),
        help: help.into(),
        trace,
        location: Some(location.to_owned()),
    }
}

#[cfg(feature = "experimental-model-check")]
fn sort_diagnostics(diagnostics: &mut [CheckDiagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.code
            .cmp(right.code)
            .then_with(|| left.subject.cmp(&right.subject))
    });
}

/// Checker descriptor registered automatically by modeled derives and mappings.
#[cfg(feature = "experimental-model-check")]
#[doc(hidden)]
pub enum DescriptorKind {
    Field {
        role: &'static str,
        field: &'static str,
        root: bool,
        location: &'static str,
    },
    Mapping {
        name: &'static str,
        sources: &'static [&'static str],
        target: &'static str,
        temporal_sources: &'static [bool],
        location: &'static str,
    },
    Assumption {
        name: &'static str,
        target: &'static str,
        location: &'static str,
    },
}

/// A checker descriptor registered in the linked program image.
#[cfg(feature = "experimental-model-check")]
#[doc(hidden)]
pub struct Descriptor {
    kind: DescriptorKind,
}

#[cfg(feature = "experimental-model-check")]
impl Descriptor {
    #[doc(hidden)]
    #[must_use]
    pub const fn field(role: &'static str, field: &'static str, root: bool) -> Self {
        Self::field_at(role, field, root, "<explicit descriptor>")
    }

    #[doc(hidden)]
    #[must_use]
    pub const fn field_at(
        role: &'static str,
        field: &'static str,
        root: bool,
        location: &'static str,
    ) -> Self {
        Self {
            kind: DescriptorKind::Field {
                role,
                field,
                root,
                location,
            },
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub const fn mapping(
        name: &'static str,
        sources: &'static [&'static str],
        target: &'static str,
        temporal_sources: &'static [bool],
    ) -> Self {
        Self::mapping_at(
            name,
            sources,
            target,
            temporal_sources,
            "<explicit descriptor>",
        )
    }

    #[doc(hidden)]
    #[must_use]
    pub const fn mapping_at(
        name: &'static str,
        sources: &'static [&'static str],
        target: &'static str,
        temporal_sources: &'static [bool],
        location: &'static str,
    ) -> Self {
        Self {
            kind: DescriptorKind::Mapping {
                name,
                sources,
                target,
                temporal_sources,
                location,
            },
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub const fn assumption(name: &'static str, target: &'static str) -> Self {
        Self::assumption_at(name, target, "<explicit descriptor>")
    }

    #[doc(hidden)]
    #[must_use]
    pub const fn assumption_at(
        name: &'static str,
        target: &'static str,
        location: &'static str,
    ) -> Self {
        Self {
            kind: DescriptorKind::Assumption {
                name,
                target,
                location,
            },
        }
    }

    fn stable_id(&self) -> &'static str {
        match self.kind {
            DescriptorKind::Field { field, .. } => field,
            DescriptorKind::Mapping { name, .. } => name,
            DescriptorKind::Assumption { name, .. } => name,
        }
    }

    fn location(&self) -> &'static str {
        match self.kind {
            DescriptorKind::Field { location, .. }
            | DescriptorKind::Mapping { location, .. }
            | DescriptorKind::Assumption { location, .. } => location,
        }
    }
}

#[cfg(feature = "experimental-model-check")]
inventory::collect!(Descriptor);

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "experimental-model-check")]
    use std::collections::BTreeMap;

    #[derive(Debug)]
    struct InitialState;

    impl ModelState for InitialState {
        fn initial() -> Modeled<Self> {
            Modeled::from_built(Self)
        }
    }

    #[test]
    fn modeled_state_default_uses_model_initialization() {
        let state = Modeled::<InitialState>::default();

        assert!(matches!(state.into_inner(), InitialState));
    }

    #[cfg(feature = "experimental-model-check")]
    #[test]
    fn temporal_recurrence_requires_a_non_temporal_seed() {
        static INPUT: Descriptor = Descriptor::field("input", "Input.amount", true);
        static BALANCE: Descriptor = Descriptor::field("read_model", "History.balance", false);
        static RECURRENCE: Descriptor = Descriptor::mapping(
            "CreditBalance",
            &["History.balance", "Input.amount"],
            "History.balance",
            &[true, false],
        );

        let fields = BTreeMap::from([("Input.amount", &INPUT), ("History.balance", &BALANCE)]);
        let mappings = BTreeMap::from([("History.balance", vec![&RECURRENCE])]);
        let assumptions = BTreeMap::new();
        let options = CheckOptions::default();
        let graph = CheckerGraph {
            fields: &fields,
            mappings: &mappings,
            assumptions: &assumptions,
            options: &options,
        };
        let mut evaluation = CheckerEvaluation::default();

        assert!(!is_complete("History.balance", &graph, &mut evaluation));
        assert!(evaluation.errors.iter().any(|error| error.code == "ECM007"));
    }

    #[cfg(feature = "experimental-model-check")]
    #[test]
    fn temporal_recurrence_accepts_a_non_temporal_seed() {
        static INPUT: Descriptor = Descriptor::field("input", "Input.amount", true);
        static BALANCE: Descriptor = Descriptor::field("read_model", "History.balance", false);
        static RECURRENCE: Descriptor = Descriptor::mapping(
            "CreditBalance",
            &["History.balance", "Input.amount"],
            "History.balance",
            &[true, false],
        );
        static SEED: Descriptor = Descriptor::mapping(
            "InitialBalance",
            &["Input.amount"],
            "History.balance",
            &[false],
        );

        let fields = BTreeMap::from([("Input.amount", &INPUT), ("History.balance", &BALANCE)]);
        let mappings = BTreeMap::from([("History.balance", vec![&RECURRENCE, &SEED])]);
        let assumptions = BTreeMap::new();
        let options = CheckOptions::default();
        let graph = CheckerGraph {
            fields: &fields,
            mappings: &mappings,
            assumptions: &assumptions,
            options: &options,
        };
        let mut evaluation = CheckerEvaluation::default();

        assert!(is_complete("History.balance", &graph, &mut evaluation));
        assert!(evaluation.errors.is_empty());
    }

    #[cfg(feature = "experimental-model-check")]
    #[test]
    fn explicit_descriptor_checks_retain_duplicate_registration_errors() {
        let descriptors = [
            Descriptor::field("input", "Input.amount", true),
            Descriptor::field("input", "Input.amount", true),
        ];

        let error = check_descriptors(&descriptors, CheckOptions::default())
            .expect_err("duplicate descriptors must not be discarded during evaluation");
        assert!(error.diagnostics.iter().any(|error| error.code == "ECM002"));
    }

    #[cfg(feature = "experimental-model-check")]
    #[test]
    fn multi_input_mapping_requires_every_source_and_every_alternative() {
        let complete = [
            Descriptor::field("input", "Input.left", true),
            Descriptor::field("input", "Input.right", true),
            Descriptor::field("output", "Output.sum", false),
            Descriptor::mapping(
                "Sum",
                &["Input.left", "Input.right"],
                "Output.sum",
                &[false, false],
            ),
        ];
        assert_eq!(
            check_descriptors(&complete, CheckOptions::default())
                .expect("both sources make the AND-edge complete")
                .status,
            CheckStatus::Verified
        );

        let incomplete_alternative = [
            Descriptor::field("input", "Input.left", true),
            Descriptor::field("output", "Output.sum", false),
            Descriptor::mapping("Good", &["Input.left"], "Output.sum", &[false]),
            Descriptor::mapping("Broken", &["Missing.right"], "Output.sum", &[false]),
        ];
        let error = check_descriptors(&incomplete_alternative, CheckOptions::default())
            .expect_err("all registered producer alternatives must be complete");
        assert!(
            error
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ECM004")
        );
    }

    #[cfg(feature = "experimental-model-check")]
    #[test]
    fn checker_reports_empty_registry_and_unused_boundaries() {
        let empty = check_descriptors(&[], CheckOptions::default())
            .expect_err("an empty explicit descriptor set is not a model");
        assert_eq!(empty.diagnostics[0].code, "ECM001");

        let descriptors = [
            Descriptor::field("input", "Input.used", true),
            Descriptor::field("input", "Input.unused", true),
            Descriptor::field("event", "Event.value", false),
            Descriptor::field("output", "Output.value", false),
            Descriptor::mapping("EventFromInput", &["Input.used"], "Event.value", &[false]),
            Descriptor::mapping("OutputFromInput", &["Input.used"], "Output.value", &[false]),
        ];
        let report = check_descriptors(&descriptors, CheckOptions::default())
            .expect("the output remains complete even with unused boundaries");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "ECM102")
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "ECM103")
        );
    }
}
