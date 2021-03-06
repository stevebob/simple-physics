use aabb::Aabb;
use axis_aligned_rect::AxisAlignedRect;
use cgmath::{vec2, ElementWise, InnerSpace, Vector2};
use fnv::{FnvHashMap, FnvHashSet};
use line_segment::LineSegment;
use loose_quad_tree::LooseQuadTree;
use movement::{Displacement, EntityId, ForEachShapePosition, MovementContext};
use shape::{Shape, ShapePosition};
use std::collections::HashMap;

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

#[derive(Debug)]
pub struct InputModel {
    left: f64,
    right: f64,
    up: f64,
    down: f64,
    jump_current: bool,
    jump_count: Option<u64>,
}

impl Default for InputModel {
    fn default() -> Self {
        Self {
            left: 0.,
            right: 0.,
            up: 0.,
            down: 0.,
            jump_current: false,
            jump_count: None,
        }
    }
}

impl InputModel {
    pub fn set_jump(&mut self, jump: bool) {
        self.jump_current = jump;
    }
    pub fn set_left(&mut self, value: f64) {
        self.left = clamp(value, 0., 1.);
    }
    pub fn set_right(&mut self, value: f64) {
        self.right = clamp(value, 0., 1.);
    }
    pub fn set_up(&mut self, value: f64) {
        self.up = clamp(value, 0., 1.);
    }
    pub fn set_down(&mut self, value: f64) {
        self.down = clamp(value, 0., 1.);
    }
    fn horizontal(&self) -> f64 {
        self.right - self.left
    }
    fn vertical(&self) -> f64 {
        self.down - self.up
    }
    fn movement(&self) -> Vector2<f64> {
        let raw = vec2(self.horizontal(), self.vertical());
        if raw.magnitude2() > 1. {
            raw.normalize()
        } else {
            raw
        }
    }
    pub fn after_process(&mut self) {
        if self.jump_current {
            self.jump_count = match self.jump_count {
                Some(count) => Some(count + 1),
                None => Some(0),
            }
        } else {
            self.jump_count = None;
        }
    }
}

pub struct RenderUpdate<'a> {
    pub position: Vector2<f64>,
    pub shape: &'a Shape,
    pub colour: [f32; 3],
}

fn jump_frame_count_to_velocity(count: u64) -> Option<f64> {
    const MAX_COUNT: u64 = 6;
    const MULTIPLIER: f64 = 0.4;
    if count >= MAX_COUNT {
        None
    } else {
        Some(((MAX_COUNT - count) as f64) * MULTIPLIER)
    }
}

fn update_player_velocity(
    current_velocity: Vector2<f64>,
    input_model: &InputModel,
    max_platform_velocity: Option<Vector2<f64>>,
    jump: &JumpStateMachine,
) -> Vector2<f64> {
    const MULTIPLIER: Vector2<f64> = Vector2 { x: 4., y: 0.5 };
    const GRAVITY: Vector2<f64> = Vector2 { x: 0., y: 0.5 };
    const MAX_LATERAL: f64 = 10.;
    const DECAY: Vector2<f64> = Vector2 { x: 0.0, y: 1. };

    let (current_velocity_relative, platform_velocity) =
        if let Some(max_platform_velocity) = max_platform_velocity {
            let current_velocity_relative =
                (current_velocity - max_platform_velocity).mul_element_wise(DECAY);
            (current_velocity_relative, max_platform_velocity)
        } else {
            (current_velocity.mul_element_wise(DECAY), vec2(0., 0.))
        };

    let input_movement = input_model.movement().mul_element_wise(MULTIPLIER);

    let horizontal_velocity_relative = clamp(
        current_velocity_relative.x + input_movement.x,
        -MAX_LATERAL,
        MAX_LATERAL,
    );

    let vertical_delta = match jump {
        JumpStateMachine::NotJumping => GRAVITY,
        JumpStateMachine::JumpingForFrames(n) => match jump_frame_count_to_velocity(*n) {
            Some(y) => vec2(0., -y),
            None => GRAVITY,
        },
    };
    let vertical_velocity_relative = current_velocity_relative.y + vertical_delta.y;

    let velocity_relative = vec2(
        horizontal_velocity_relative,
        vertical_velocity_relative,
    );

    platform_velocity + velocity_relative
}

#[derive(Default)]
struct EntityIdAllocator {
    next: u32,
}

impl EntityIdAllocator {
    fn allocate(&mut self) -> EntityId {
        let id = self.next;
        self.next += 1;
        id
    }
    fn reset(&mut self) {
        self.next = 0;
    }
}

#[derive(Debug)]
struct EntityCommon {
    position: Vector2<f64>,
    shape: Shape,
    colour: [f32; 3],
}

impl EntityCommon {
    fn new(position: Vector2<f64>, shape: Shape, colour: [f32; 3]) -> Self {
        Self {
            position,
            shape,
            colour,
        }
    }
    fn aabb(&self) -> Aabb {
        self.shape.aabb(self.position)
    }
}

#[derive(Default)]
pub struct GameStateChanges {
    position: Vec<(EntityId, Vector2<f64>)>,
    velocity: HashMap<EntityId, Vector2<f64>>,
    displacements: Vec<(EntityId, Displacement)>,
}

enum JumpStateMachine {
    NotJumping,
    JumpingForFrames(u64),
}

impl JumpStateMachine {
    pub fn step(&mut self, can_jump: bool, input: &InputModel) {
        if let Some(jump_count) = input.jump_count {
            if jump_count == 0 {
                if can_jump {
                    *self = JumpStateMachine::JumpingForFrames(0);
                } else {
                    *self = JumpStateMachine::NotJumping;
                }
            } else {
                match self {
                    JumpStateMachine::NotJumping => (),
                    JumpStateMachine::JumpingForFrames(ref mut n) => *n += 1,
                }
            }
        } else {
            *self = JumpStateMachine::NotJumping;
        }
    }
}

pub struct GameState {
    player_id: Option<EntityId>,
    moving_platform_ids: Vec<EntityId>,
    entity_id_allocator: EntityIdAllocator,
    common: FnvHashMap<EntityId, EntityCommon>,
    velocity: FnvHashMap<EntityId, Vector2<f64>>,
    dynamic_physics: FnvHashSet<EntityId>,
    static_physics: FnvHashSet<EntityId>,
    quad_tree: LooseQuadTree<EntityId>,
    jump: FnvHashMap<EntityId, JumpStateMachine>,
    frame_count: u64,
}

struct AllShapePositions<'a>(&'a GameState);
struct DynamicPhysicsShapePositions<'a>(&'a GameState);

impl<'a> ForEachShapePosition for AllShapePositions<'a> {
    fn for_each<F: FnMut(ShapePosition)>(&self, aabb: Aabb, mut f: F) {
        self.0
            .quad_tree
            .for_each_intersection(aabb, |_aabb, &entity_id| {
                let common = self.0.common.get(&entity_id).unwrap();
                let shape_position = ShapePosition {
                    entity_id,
                    shape: &common.shape,
                    position: common.position,
                };
                f(shape_position);
            });
    }
}

impl<'a> ForEachShapePosition for DynamicPhysicsShapePositions<'a> {
    fn for_each<F: FnMut(ShapePosition)>(&self, aabb: Aabb, mut f: F) {
        self.0
            .quad_tree
            .for_each_intersection(aabb, |_aabb, &entity_id| {
                if self.0.dynamic_physics.contains(&entity_id) {
                    let common = self.0.common.get(&entity_id).unwrap();
                    let shape_position = ShapePosition {
                        entity_id,
                        shape: &common.shape,
                        position: common.position,
                    };
                    f(shape_position);
                }
            });
    }
}

impl GameState {
    pub fn new(size_hint: Vector2<f64>) -> Self {
        Self {
            player_id: None,
            moving_platform_ids: Vec::new(),
            entity_id_allocator: Default::default(),
            common: Default::default(),
            velocity: Default::default(),
            dynamic_physics: Default::default(),
            static_physics: Default::default(),
            quad_tree: LooseQuadTree::new(size_hint),
            jump: Default::default(),
            frame_count: 0,
        }
    }
    fn clear(&mut self) {
        self.player_id = None;
        self.entity_id_allocator.reset();
        self.common.clear();
        self.velocity.clear();
        self.dynamic_physics.clear();
        self.static_physics.clear();
        self.quad_tree.clear();
        self.jump.clear();
        self.frame_count = 0;
    }
    fn add_static_solid(&mut self, common: EntityCommon) -> EntityId {
        let id = self.entity_id_allocator.allocate();
        self.quad_tree.insert(common.aabb(), id);
        self.common.insert(id, common);
        id
    }
    fn add_common(&mut self, common: EntityCommon) -> EntityId {
        let id = self.entity_id_allocator.allocate();
        self.common.insert(id, common);
        id
    }
    pub fn init_demo(&mut self) {
        self.clear();
        let player_id = self.add_common(EntityCommon::new(
            vec2(550., 500. - 64.),
            Shape::AxisAlignedRect(AxisAlignedRect::new_character(vec2(32., 64.))),
            [1., 0., 0.],
        ));
        self.player_id = Some(player_id);
        self.velocity.insert(player_id, vec2(0., 0.));
        self.dynamic_physics.insert(player_id);
        self.jump
            .insert(player_id, JumpStateMachine::NotJumping);
        let moving_platform_id = self.add_static_solid(EntityCommon::new(
            vec2(200., 350.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(128., 32.))),
            [0., 1., 1.],
        ));
        self.moving_platform_ids.push(moving_platform_id);
        self.velocity.insert(moving_platform_id, vec2(0., 0.));
        self.static_physics.insert(moving_platform_id);

        let moving_platform_id = self.add_static_solid(EntityCommon::new(
            vec2(700., 450.),
            Shape::LineSegment(LineSegment::new_both_solid(
                vec2(0., 32.),
                vec2(128., 0.),
            )),
            [0., 1., 1.],
        ));
        self.moving_platform_ids.push(moving_platform_id);
        self.velocity.insert(moving_platform_id, vec2(0., 0.));
        self.static_physics.insert(moving_platform_id);

        self.add_static_solid(EntityCommon::new(
            vec2(700., 200.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(32., 64.))),
            [1., 1., 0.],
        ));

        self.add_static_solid(EntityCommon::new(
            vec2(50., 200.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(400., 20.))),
            [1., 1., 0.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(150., 250.),
            Shape::AxisAlignedRect(AxisAlignedRect::new_floor_only(vec2(500., 20.))),
            [1., 1., 1.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(50., 450.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(100., 20.))),
            [1., 1., 0.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(50., 500.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(700., 20.))),
            [1., 1., 0.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(450., 499.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(20., 20.))),
            [1., 1., 0.],
        ));

        self.add_static_solid(EntityCommon::new(
            vec2(600., 498.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(20., 20.))),
            [1., 1., 0.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(620., 496.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(20., 20.))),
            [1., 1., 0.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(640., 492.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(20., 20.))),
            [1., 1., 0.],
        ));

        self.add_static_solid(EntityCommon::new(
            vec2(760., 500.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(20., 20.))),
            [1., 1., 0.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(813., 500.),
            Shape::AxisAlignedRect(AxisAlignedRect::new(vec2(20., 20.))),
            [1., 1., 0.],
        ));

        self.add_static_solid(EntityCommon::new(
            vec2(20., 20.),
            Shape::LineSegment(LineSegment::new_both_solid(
                vec2(0., 0.),
                vec2(50., 100.),
            )),
            [0., 1., 0.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(200., 20.),
            Shape::LineSegment(LineSegment::new_both_solid(
                vec2(0., 0.),
                vec2(300., 200.),
            )),
            [0., 1., 0.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(200., 20.),
            Shape::LineSegment(LineSegment::new_both_solid(
                vec2(0., 120.),
                vec2(300., 200.),
            )),
            [0., 1., 0.],
        ));
        self.add_static_solid(EntityCommon::new(
            vec2(900., 200.),
            Shape::LineSegment(LineSegment::new_both_solid(
                vec2(0., 0.),
                vec2(-300., 200.),
            )),
            [0., 1., 0.],
        ));

        let moving_platform_id = self.add_static_solid(EntityCommon::new(
            vec2(300., 472.),
            Shape::LineSegment(LineSegment::new_both_solid(
                vec2(0., 0.),
                vec2(32., 32.),
            )),
            [0., 1., 0.],
        ));
        self.moving_platform_ids.push(moving_platform_id);
        self.velocity.insert(moving_platform_id, vec2(0., 0.));
        self.static_physics.insert(moving_platform_id);
    }
    pub fn update(
        &mut self,
        input_model: &InputModel,
        changes: &mut GameStateChanges,
        movement_context: &mut MovementContext,
    ) {
        self.quad_tree.clear();
        for (id, common) in self.common.iter() {
            self.quad_tree.insert(common.aabb(), *id);
        }

        self.velocity.insert(
            self.moving_platform_ids[0],
            vec2(((self.frame_count as f64) * 0.05).sin() * 2., 0.),
        );
        self.velocity.insert(
            self.moving_platform_ids[1],
            vec2(0., ((self.frame_count as f64) * 0.1).sin() * 4.),
        );
        self.velocity.insert(
            self.moving_platform_ids[2],
            vec2(((self.frame_count as f64) * 0.1).sin() * 5., 0.),
        );

        let player_id = self.player_id.expect("No player id");
        {
            let collisions_below_player = {
                let player_common = self.common.get(&player_id).unwrap();
                let player_shape_position = ShapePosition {
                    entity_id: player_id,
                    position: player_common.position,
                    shape: &player_common.shape,
                };

                movement_context
                    .collisions_below(player_shape_position, &AllShapePositions(self))
            };

            let jump = self.jump
                .get_mut(&player_id)
                .expect("No jump for player");

            jump.step(collisions_below_player.can_jump(), input_model);

            let max_platform_velocity = {
                let velocity = &mut self.velocity;
                collisions_below_player.max_velocity(|id| velocity.get(&id).cloned())
            };

            if let Some(velocity) = self.velocity.get_mut(&player_id) {
                *velocity = update_player_velocity(
                    *velocity,
                    input_model,
                    max_platform_velocity,
                    jump,
                );
            }
        }

        for id in self.dynamic_physics.iter() {
            if let Some(velocity) = self.velocity.get(id) {
                if let Some(common) = self.common.get(id) {
                    let shape_position = ShapePosition {
                        entity_id: *id,
                        position: common.position,
                        shape: &common.shape,
                    };
                    let movement = movement_context.position_after_allowed_movement(
                        shape_position,
                        *velocity,
                        &AllShapePositions(self),
                    );
                    changes.velocity.insert(*id, movement.velocity);
                    changes.position.push((*id, movement.position));
                }
            }
        }

        for (id, position) in changes.position.drain(..) {
            if let Some(common) = self.common.get_mut(&id) {
                common.position = position;
            }
        }

        for (id, velocity) in changes.velocity.drain() {
            self.velocity.insert(id, velocity);
        }

        self.quad_tree.clear();
        for (id, common) in self.common.iter() {
            self.quad_tree.insert(common.aabb(), *id);
        }

        for id in self.static_physics.iter() {
            if let Some(velocity) = self.velocity.get(id) {
                if let Some(common) = self.common.get(id) {
                    let shape_position = ShapePosition {
                        entity_id: *id,
                        position: common.position,
                        shape: &common.shape,
                    };
                    movement_context.displacement_after_movement(
                        shape_position,
                        *velocity,
                        &DynamicPhysicsShapePositions(self),
                        &mut changes.displacements,
                    );
                    changes
                        .position
                        .push((*id, common.position + velocity));
                }
            }
        }

        for (id, displacement) in changes.displacements.drain(..) {
            if let Some(common) = self.common.get_mut(&id) {
                common.position += displacement.movement;
            }
            if let Some(velocity) = self.velocity.get_mut(&id) {
                *velocity = displacement.combine_velocity(*velocity);
            }
        }

        for (id, position) in changes.position.drain(..) {
            if let Some(common) = self.common.get_mut(&id) {
                common.position = position;
            }
        }

        self.frame_count += 1;
    }
    pub fn render_updates(&self) -> impl Iterator<Item = RenderUpdate> {
        self.common.values().map(|common| RenderUpdate {
            position: common.position,
            shape: &common.shape,
            colour: common.colour,
        })
    }
}
