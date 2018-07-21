use aabb::Aabb;
use cgmath::{InnerSpace, Vector2, vec2};
use collide::Collide;
use left_solid_edge::{LeftSolidEdge, vector2_cross_product, EPSILON};

#[derive(Debug, Clone, Copy)]
pub enum SolidSide {
    Left,
    Both,
}

#[derive(Debug, Clone, Copy)]
pub struct LineSegment {
    pub start: Vector2<f32>,
    pub end: Vector2<f32>,
    pub solid_side: SolidSide,
}

impl LineSegment {
    pub fn new_both_solid(start: Vector2<f32>, end: Vector2<f32>) -> Self {
        Self {
            start,
            end,
            solid_side: SolidSide::Both,
        }
    }
    pub fn new_left_solid(start: Vector2<f32>, end: Vector2<f32>) -> Self {
        Self {
            start,
            end,
            solid_side: SolidSide::Left,
        }
    }
    pub fn add_vector(&self, vector: Vector2<f32>) -> Self {
        Self {
            start: self.start + vector,
            end: self.end + vector,
            solid_side: self.solid_side,
        }
    }
    pub fn vector(&self) -> Vector2<f32> {
        self.end - self.start
    }
    fn left_solid_edge(&self) -> LeftSolidEdge {
        LeftSolidEdge::new(self.start, self.end)
    }
    fn left_solid_edge_flipped(&self) -> LeftSolidEdge {
        LeftSolidEdge::new(self.end, self.start)
    }
}

impl Collide for LineSegment {
    fn aabb(&self, top_left: Vector2<f32>) -> Aabb {
        let start = self.start + top_left;
        let end = self.end + top_left;
        let x_min = start.x.min(end.x);
        let x_max = start.x.max(end.x);
        let y_min = start.y.min(end.y);
        let y_max = start.y.max(end.y);
        let top_left = vec2(x_min, y_min);
        let bottom_right = vec2(x_max, y_max);
        Aabb::new(top_left, bottom_right - top_left)
    }
    fn for_each_edge_facing<F: FnMut(LineSegment)>(
        &self,
        _direction: Vector2<f32>,
        mut f: F,
    ) {
        f(*self);
    }
    fn for_each_vertex_facing<F: FnMut(Vector2<f32>)>(
        &self,
        direction: Vector2<f32>,
        mut f: F,
    ) {
        self.for_each_left_solid_edge_facing(direction, |edge| {
            f(edge.start);
            f(edge.end);
        });
    }

    fn for_each_left_solid_edge_facing<F: FnMut(LeftSolidEdge)>(
        &self,
        direction: Vector2<f32>,
        mut f: F,
    ) {
        let vector = self.vector();
        let left = vec2(-vector.y, vector.x).normalize();
        f(self.add_vector(left).left_solid_edge_flipped());
        f(self.add_vector(-left).left_solid_edge());
    }
}
