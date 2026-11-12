//! `Animal.mobInteract` food acceptance.
use super::{Mob, Species, can_age_up};

/// Whether `Animal.mobInteract` takes the animal's food: an adult at age 0
/// that `canFallInLove` (not in love yet) or a baby that `canAgeUp` (not age
/// locked). Tamed wolves and cats only sit or stand when it does not.
pub fn takes_food(m: &Mob) -> bool {
    // `canFallInLove`: a turtle carrying an egg and a pacified hoglin refuse their food.
    let breedable = m.extra.in_love <= 0
        && !(m.species() == Species::Turtle && crate::mobs::turtle::has_egg(m))
        && !crate::mobs::piglin::refuses_love(m);
    // The growing branch is `canAgeUp()`, which asks `isBaby()`, not the raw
    // age: a frog or a camel husk that `canBeABaby` says is never a baby keeps
    // the food in the player's hand however negative its `Age` is.
    (m.extra.age == 0 && breedable) || can_age_up(m)
}
