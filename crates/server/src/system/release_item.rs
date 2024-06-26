use bvh_region::aabb::Aabb;
use evenio::prelude::*;
use glam::Vec3;
use tracing::instrument;
use valence_protocol::{packets::play, ByteAngle, VarInt, Velocity};
use valence_server::entity::EntityKind;

use crate::{
    components::{Arrow, EntityPhysics, EntityPhysicsState, FullEntityPose, Uuid},
    event::ReleaseItem,
    inventory::PlayerInventory,
    net::Compose,
    system::sync_entity_position::PositionSyncMetadata,
};

#[derive(Query)]
pub struct ReleaseItemQuery<'a> {
    pose: &'a FullEntityPose,
    inventory: &'a mut PlayerInventory,
}

#[instrument(skip_all, level = "trace")]
pub fn release_item(
    r: Receiver<ReleaseItem, ReleaseItemQuery>,
    compose: Compose,
    s: Sender<(
        Insert<Arrow>,
        Insert<EntityPhysics>,
        Insert<FullEntityPose>,
        Insert<PositionSyncMetadata>,
        Insert<Uuid>,
        Spawn,
    )>,
) {
    // TODO: Check that there is a bow and arrow
    tracing::info!("shoot arrow");

    let query = r.query;

    let id = s.spawn();

    #[expect(clippy::cast_possible_wrap, reason = "wrapping is ok in this case")]
    let entity_id = VarInt(id.index().0 as i32);

    let uuid = Uuid::from(uuid::Uuid::new_v4());

    let duration = query.inventory.interact_duration().unwrap().as_secs_f32();
    // TODO: Eyeballed this number, verify correctness
    let initial_speed = f32::min(duration, 1.0) * 4.22;

    let (pitch_sin, pitch_cos) = query.pose.pitch.to_radians().sin_cos();
    let (yaw_sin, yaw_cos) = query.pose.yaw.to_radians().sin_cos();
    let velocity = Vec3::new(-pitch_cos * yaw_sin, -pitch_sin, pitch_cos * yaw_cos) * initial_speed;
    let encoded_velocity = Velocity(velocity.to_array().map(|a| (a * 8000.0) as i16));

    let position = query.pose.position + Vec3::new(0.0, 1.52, 0.0);

    s.insert(id, Arrow);
    s.insert(id, EntityPhysics {
        state: EntityPhysicsState::Moving { velocity },
        gravity: 0.05,
        drag: 0.01,
    });
    s.insert(id, FullEntityPose {
        position,
        yaw: query.pose.yaw,
        pitch: query.pose.pitch,
        bounding: Aabb::create(position, 0.5, 0.5), // TODO: use correct values
    });
    s.insert(id, PositionSyncMetadata::default());
    s.insert(id, uuid);

    let pkt = play::EntitySpawnS2c {
        entity_id,
        object_uuid: *uuid,
        kind: VarInt(EntityKind::ARROW.get()),
        position: position.as_dvec3(),
        pitch: ByteAngle::from_degrees(query.pose.pitch),
        yaw: ByteAngle::from_degrees(query.pose.yaw),
        head_yaw: ByteAngle::from_degrees(0.0),
        data: VarInt::default(),
        velocity: encoded_velocity,
    };

    compose.broadcast(&pkt).send().unwrap();

    // At least one velocity packet is needed for the arrow to not immediately fall to the ground,
    // so one velocity packet needs to be sent manually instead of relying on
    // sync_entity_velocity.rs in case the arrow hits a wall on the same tick.
    let pkt = play::EntityVelocityUpdateS2c {
        entity_id,
        velocity: encoded_velocity,
    };

    compose.broadcast(&pkt).send().unwrap();
}
