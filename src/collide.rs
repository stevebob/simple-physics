use aabb::Aabb;
use best::BestSet;
use cgmath::Vector2;
use left_solid_edge::{CollisionMovement, LeftSolidEdge, MovementWithSlide};

pub trait Collide {
    fn aabb(&self, top_left: Vector2<f64>) -> Aabb;
    fn for_each_left_solid_edge_facing<F: FnMut(LeftSolidEdge)>(
        &self,
        direction: Vector2<f64>,
        f: F,
    );

    fn for_each_movement_collision_<StationaryShape, F>(
        &self,
        position: Vector2<f64>,
        stationary_shape: &StationaryShape,
        stationary_position: Vector2<f64>,
        movement: Vector2<f64>,
        mut f: F,
    ) where
        Self: Sized,
        StationaryShape: Collide + ::std::fmt::Debug,
        F: FnMut(CollisionMovement),
    {
        self.for_each_left_solid_edge_facing(movement, |rel_edge| {
            let moving_edge = rel_edge.add_vector(position);
            stationary_shape.for_each_left_solid_edge_facing(-movement, |rel_edge| {
                let stationary_edge = rel_edge.add_vector(stationary_position);
                let collision_movement =
                    moving_edge.collide_with_stationary_edge(&stationary_edge, movement);
                f(collision_movement);
            });
        });
    }
    fn movement_collision_test_<StationaryShape>(
        &self,
        position: Vector2<f64>,
        stationary_shape: &StationaryShape,
        stationary_position: Vector2<f64>,
        movement: Vector2<f64>,
    ) -> MovementWithSlide
    where
        Self: Sized,
        StationaryShape: Collide + ::std::fmt::Debug,
    {
        let mut shortest_movement = BestSet::new();

        self.for_each_movement_collision_(
            position,
            stationary_shape,
            stationary_position,
            movement,
            |collision_movement| {
                shortest_movement.insert_le(collision_movement);
            },
        );

        shortest_movement
            .into_value()
            .map(|c| c.movement)
            .unwrap_or_else(|| MovementWithSlide::new_just_movement(movement))
    }
}
