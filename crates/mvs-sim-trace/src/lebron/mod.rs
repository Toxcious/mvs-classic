//! LeBron's actors as the record writes them (`chars/lebron/`): a file per port track, each with its classes' `put`
//! (the class's own keys into the actor's `rest`, its place into the record) and `load` (the inverse; the keys it
//! knows into `known`). `actors.rs` calls them from its arms.

pub mod ball;
pub mod bubble;
pub mod later;
