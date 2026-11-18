//! The build's collision profiles as the queries read them.

/// DefaultEngine.ini [/Script/Engine.CollisionProfile] +Profiles 551-594: ObjectTypeName as a query index (an empty
/// one: the serialized WorldStatic) and whether CollisionEnabled has the query on.
/// Unverified: an empty ObjectTypeName taken as WorldStatic.
pub const PROFILES: [(&str, i32, bool); 44] = [
    ("NoCollision", 0, false),
    ("BlockAll", 0, true),
    ("OverlapAll", 0, true),
    ("BlockAllDynamic", 1, true),
    ("OverlapAllDynamic", 1, true),
    ("IgnoreOnlyPawn", 1, true),
    ("OverlapOnlyPawn", 1, true),
    ("Pawn", 2, true),
    ("Spectator", 2, true),
    ("CharacterMesh", 2, true),
    ("PhysicsActor", 3, true),
    ("Destructible", 5, true),
    ("InvisibleWall", 0, true),
    ("InvisibleWallDynamic", 1, true),
    ("Trigger", 1, true),
    ("Ragdoll", 3, true),
    ("Vehicle", 4, true),
    ("UI", 1, true),
    ("HitBox", 7, false),
    ("PlatformDetection", 6, true),
    ("DamageDestructible", 8, true),
    ("InvisibleWallPawn", 0, true),
    ("KnockBack", 0, true),
    ("Platform", 10, true),
    ("CharacterPhysicsActor", 3, true),
    ("DetectPawn", 1, true),
    ("Ghost", 8, true),
    ("Tether", 1, true),
    ("Projectile", 7, false),
    ("ProjectileDestructible", 11, true),
    ("IgnorePlatform", 2, true),
    ("Floor", 0, true),
    ("Fog", 1, true),
    ("NonDamageDestructibleHitBox", 7, false),
    ("Item", 14, true),
    ("PortalOverlap", 15, true),
    ("PortalProjectile", 1, true),
    ("BlockPortal", 0, true),
    ("ProjectileBreaking", 3, true),
    ("Physmat", 0, true),
    ("ItemDestructible", 16, true),
    ("ProjectileNavNoPlatform", 3, true),
    ("ProjectileNavPlatform", 3, true),
    ("HurtBox", 18, true),
];

/// A profile's row: its object type (query index) and whether its query is on; `None` for one the table lacks.
pub fn profile(name: &str) -> Option<(i32, bool)> {
    PROFILES.iter().find(|p| p.0 == name).map(|p| (p.1, p.2))
}

/// Each profile's response to the object types in query index order (the sim's OBJ_NAMES: WorldStatic .. BlockTrace),
/// B Block, O Overlap, I Ignore: DefaultEngine.ini's DefaultChannelResponses under the profile's CustomResponses and
/// EditProfiles (a response left out of an entry is Block). The build applies it over a body's cooked ResponseArray
/// unless the profile is Custom (FBodyInstance::FixupData 142c714f0, LoadProfileData 142c77210).
const RESPONSES: [(&str, &str); 44] = [
    ("NoCollision", "IIIIIIIIIIIIIIIIIIII"),            // NoCollision WorldStatic
    ("BlockAll", "BBBBBBIIBIBIIIBIIBII"),               // QueryAndPhysics WorldStatic
    ("OverlapAll", "OOOOOOIIBIBIIIOIIOII"),             // QueryOnly WorldStatic
    ("BlockAllDynamic", "BBBBBBIIBIBIIIBIIIII"),        // QueryAndPhysics WorldDynamic
    ("OverlapAllDynamic", "OOOOOOIIBIBIIIOIIIII"),      // QueryOnly WorldDynamic
    ("IgnoreOnlyPawn", "BBIBIBIIBIBIIIBIIIII"),         // QueryOnly WorldDynamic
    ("OverlapOnlyPawn", "BBOBOBIIBIBIIIBIIIII"),        // QueryOnly WorldDynamic
    ("Pawn", "BBOBBBBOIIBIIIBOIIII"),                   // QueryAndPhysics Pawn
    ("Spectator", "BIIIIIIIBIBIIIIIIIII"),              // QueryOnly Pawn
    ("CharacterMesh", "BBIBIBIIBIBIIIOIIIII"),          // QueryOnly Pawn
    ("PhysicsActor", "BBBBIBIOBIBIIIBIIIII"),           // QueryAndPhysics PhysicsBody
    ("Destructible", "BBBBBBIIBIBIIIBIIIII"),           // QueryAndPhysics Destructible
    ("InvisibleWall", "BBBBBBIIBIBIIIOIIIII"),          // QueryAndPhysics WorldStatic
    ("InvisibleWallDynamic", "BBBBBBIIBIBIIIBIIIII"),   // QueryAndPhysics WorldDynamic
    ("Trigger", "OOOOOOIIOIOIIIOIIIII"),                // QueryOnly WorldDynamic
    ("Ragdoll", "BBIBBBIIBIBIIIBIIIII"),                // QueryAndPhysics PhysicsBody
    ("Vehicle", "BBBBBBIIBIBIIIBIIIII"),                // QueryAndPhysics Vehicle
    ("UI", "OOOOOOIIBIBIIIOIIIII"),                     // QueryOnly WorldDynamic
    ("HitBox", "IIOIIIIIOIIOIIIIOIOI"),                 // NoCollision HitBox
    ("PlatformDetection", "IIIIIIOIBIBIIIIIIIII"),      // QueryOnly PlatformDetection
    ("DamageDestructible", "BBIBIIIOBIBIIIOIIIII"),     // QueryAndPhysics DamageDestructible
    ("InvisibleWallPawn", "IIBIIIIIIIBIIIIIIIII"),      // QueryAndPhysics WorldStatic
    ("KnockBack", "IIIIIIIIOIBIIIIIIIII"),              // QueryAndPhysics
    ("Platform", "BBBBBBIIBBBIIIBIIBII"),               // QueryAndPhysics Platform
    ("CharacterPhysicsActor", "BBOBIBIOBIBIIIBIIIII"),  // QueryAndPhysics PhysicsBody
    ("DetectPawn", "IIOIIIIIIIIIIIIIIIII"),             // QueryOnly WorldDynamic
    ("Ghost", "BOOIIIIOIIIIIIOIIIII"),                  // QueryAndPhysics DamageDestructible
    ("Tether", "BBIIIIIIIIBIIIOIIIII"),                 // QueryAndPhysics WorldDynamic
    ("Projectile", "IIOIIIIIOIIOIIIIIIII"),             // NoCollision HitBox
    ("ProjectileDestructible", "IIIIIIIOIIIIIIIIIIII"), // QueryAndPhysics ProjectileDestructible
    ("IgnorePlatform", "BIIIIIIOIIIIIIIIIIII"),         // QueryAndPhysics Pawn
    ("Floor", "BBBBBBIIBIBOIIBIIBII"),                  // QueryAndPhysics
    ("Fog", "IIOIIIIIIIIIIIIIIIII"),                    // QueryOnly WorldDynamic
    ("NonDamageDestructibleHitBox", "IIOIIIIIIIBIIIIIIIII"), // NoCollision HitBox
    ("Item", "BBIIIBIIBIBIIIOIIIII"),                   // QueryAndPhysics Item
    ("PortalOverlap", "IOIOIOIOOIIIIIOIIIII"),          // QueryOnly Portal
    ("PortalProjectile", "BOOOOOIIBIOIIIOIIIII"),       // QueryOnly WorldDynamic
    ("BlockPortal", "IIIIIIIIIIIIIIIBIIII"),            // QueryAndPhysics WorldStatic
    ("ProjectileBreaking", "OIIIIIIIIIIIIIIIIIII"),     // QueryAndPhysics PhysicsBody
    ("Physmat", "IIIIIIIIIIIIIIIIIIII"),                // QueryOnly
    ("ItemDestructible", "IIIIIIIOIIIIIIIIIIII"),       // QueryOnly ItemDestructible
    ("ProjectileNavNoPlatform", "BBIIIIIIIIIIIIIOIIII"), // QueryAndPhysics PhysicsBody
    ("ProjectileNavPlatform", "BBIIIIIIIIBIIIIOIIII"),  // QueryAndPhysics PhysicsBody
    ("HurtBox", "OOOOOOOOIIOIIIOOIIII"),                // QueryOnly HurtBox
];

/// A named profile's response to an object type (query index): 'B', 'O' or 'I'; `None` for a profile the table lacks.
pub fn profile_response(name: &str, object_type: usize) -> Option<u8> {
    RESPONSES.iter().find(|p| p.0 == name).and_then(|p| p.1.as_bytes().get(object_type)).copied()
}

/// `profileObjectType`: a profile's object type (query index; -1 for one the
/// table lacks).
pub fn profile_object_type(name: &str) -> i32 {
    profile(name).map_or(-1, |p| p.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_types() {
        assert_eq!(profile_object_type("Item"), 14);
        assert_eq!(profile_object_type("HurtBox"), 18);
        assert_eq!(profile_object_type("nothing"), -1);
        assert_eq!(profile("HitBox"), Some((7, false)));
        // ProjectileNavNoPlatform: WorldStatic / WorldDynamic Block, Pawn Ignore (the live read of the sandwich root)
        assert_eq!(profile_response("ProjectileNavNoPlatform", 0), Some(b'B'));
        assert_eq!(profile_response("ProjectileNavNoPlatform", 1), Some(b'B'));
        assert_eq!(profile_response("ProjectileNavNoPlatform", 2), Some(b'I'));
        assert_eq!(profile_response("Custom", 0), None);
    }
}
