//! What a level or model file holds once it is read.

/// Where something stands: a 4 by 4 matrix stored column by column.
///
/// `transform[3]` is the position. This is the layout glTF and graphics cards use.
pub type Transform = [[f32; 4]; 4];

/// Everything read from one level or model file.
///
/// The lists refer to each other by position: an [`Instance`] names a mesh
/// by its index in `meshes`, a [`MeshPart`] names a material, and so on.
/// Every such index is valid.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    /// The shapes, each stored once however many times it is placed.
    pub meshes: Vec<Mesh>,
    /// Where each shape stands. A shape placed twice has two entries.
    pub instances: Vec<Instance>,
    /// How surfaces look.
    pub materials: Vec<Material>,
    /// Pictures paired with how to sample them. Empty when textures were not loaded.
    pub textures: Vec<Texture>,
    /// The decoded pictures. Empty when textures were not loaded.
    pub images: Vec<Image>,
    /// The lights placed in the file.
    pub lights: Vec<Light>,
    /// Named places with no shape, such as spawn points.
    pub markers: Vec<Marker>,
    /// The still water of the level, each body in the shape it took when
    /// it was let fill the hollow it was put in. A file has none: the
    /// engine works them out when it loads a level (`lore_physics::fill`).
    pub waters: Vec<Water>,
    /// The file's objects as a tree, for skins and animations to move.
    /// Empty when the file has neither, as a level usually has not.
    pub nodes: Vec<Node>,
    /// The skeletons of rigged meshes, such as characters and weapons.
    pub skins: Vec<Skin>,
    /// The animation clips, in the file's order.
    pub animations: Vec<Animation>,
    /// Things in the file that were skipped or changed, worded for its author.
    pub warnings: Vec<String>,
}

/// One shape, made of a part for each material it uses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// The name given in the 3D tool.
    pub name: Option<String>,
    /// The pieces of the shape. Empty if the file only held points or lines.
    pub parts: Vec<MeshPart>,
}

/// A piece of a mesh drawn with one material.
///
/// Each list has one entry per corner point; `positions` and `normals` are
/// always filled and the optional lists, when present, are the same length.
/// `joints` and `weights` are present together or not at all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshPart {
    /// Where each corner point is, in metres.
    pub positions: Vec<[f32; 3]>,
    /// Which way the surface faces at each point. Worked out from the
    /// triangles when the file has none.
    pub normals: Vec<[f32; 3]>,
    /// The direction textures run along the surface, needed for normal maps.
    /// The fourth number is 1 or -1. Worked out when the material has a
    /// normal map and the file has none.
    pub tangents: Option<Vec<[f32; 4]>>,
    /// Where each point is on its textures.
    pub uv0: Option<Vec<[f32; 2]>>,
    /// A second texture placement, usually for baked lighting.
    pub uv1: Option<Vec<[f32; 2]>>,
    /// A colour painted on each point (red, green, blue, alpha; linear).
    pub colors: Option<Vec<[f32; 4]>>,
    /// The four joints of a [`Skin`] that pull each point, strongest first,
    /// as positions in [`Skin::joints`]. Present on rigged meshes.
    pub joints: Option<Vec<[u16; 4]>>,
    /// How strongly each of those joints pulls. The four add up to 1, and a
    /// joint that does not pull has weight 0 and is listed as joint 0.
    pub weights: Option<Vec<[f32; 4]>>,
    /// The triangles: every three numbers are three points, listed
    /// anticlockwise when seen from the front.
    pub indices: Vec<u32>,
    /// Which of the scene's materials this part is drawn with.
    pub material: u32,
    /// The box that just holds every point.
    pub bounds: Bounds,
}

impl MeshPart {
    /// The triangles as corner positions, which is all collision needs.
    pub fn triangles(&self) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
        self.indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|corners| {
                let corner = |i: usize| self.positions.get(corners[i] as usize).copied();
                Some([corner(0)?, corner(1)?, corner(2)?])
            })
    }
}

/// A box lined up with the axes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bounds {
    /// The corner with the smallest x, y and z.
    pub min: [f32; 3],
    /// The corner with the largest x, y and z.
    pub max: [f32; 3],
}

/// One placement of a mesh in the scene.
///
/// In a file with skins or animations, the [`Node`] whose `instance` is this
/// one says what moves it.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// Which of the scene's meshes is placed here.
    pub mesh: u32,
    /// Where it stands, and its rotation and size.
    pub transform: Transform,
    /// The placement flips the shape (a negative scale), so its triangles
    /// run clockwise instead of anticlockwise.
    pub mirrored: bool,
    /// The name given in the 3D tool.
    pub name: Option<String>,
    /// The custom properties set in the 3D tool, as JSON text.
    pub extras: Option<String>,
}

impl Instance {
    /// Whether the player collides with it. Everything is solid unless its
    /// custom properties say otherwise: `solid` set to `false` or to 0, as
    /// a level's maker does for grass, flowers and leaves, which are drawn
    /// and cast shadows but are walked through.
    ///
    /// ```
    /// use lore_assets::Instance;
    ///
    /// let mut leaves = Instance {
    ///     mesh: 0,
    ///     transform: [[0.0; 4]; 4],
    ///     mirrored: false,
    ///     name: None,
    ///     extras: Some(r#"{"solid": false}"#.to_owned()),
    /// };
    /// assert!(!leaves.is_solid());
    /// leaves.extras = None;
    /// assert!(leaves.is_solid());
    /// ```
    #[must_use]
    pub fn is_solid(&self) -> bool {
        // Water is waded into, whatever else is said of it.
        !self.is_water() && self.ticked("solid").unwrap_or(true)
    }

    /// Whether it is the surface of water: a lake, a pond, the sea. A
    /// level's maker says so with the custom property `water` set to
    /// `true` (or 1) on a flat mesh laid where the surface is. It is drawn
    /// as water, with waves and the sky mirrored in it, casts no shadow
    /// and is not solid.
    ///
    /// The colour of its material is the colour of the water itself, and
    /// that colour's alpha how clear it is (1 is clear water). Its first
    /// UV map says how fast the water runs along x and z, in metres a
    /// second: nothing for a lake, and downstream for a creek.
    ///
    /// ```
    /// use lore_assets::Instance;
    ///
    /// let lake = Instance {
    ///     mesh: 0,
    ///     transform: [[0.0; 4]; 4],
    ///     mirrored: false,
    ///     name: None,
    ///     extras: Some(r#"{"water": true}"#.to_owned()),
    /// };
    /// assert!(lake.is_water() && !lake.is_solid());
    /// ```
    #[must_use]
    pub fn is_water(&self) -> bool {
        self.ticked("water").unwrap_or(false)
    }

    /// Whether it floats on still water: a lily pad, a leaf, a plank. A
    /// level's maker says so with the custom property `floats` set to
    /// `true` (or 1) on something laid at the water's height. It then
    /// rises and falls with the water's waves, and with the rings a
    /// wader sends out, as the water under it does. It is drawn as what
    /// it is; whether it is solid is its `solid` property's to say.
    ///
    /// ```
    /// use lore_assets::Instance;
    ///
    /// let pad = Instance {
    ///     mesh: 0,
    ///     transform: [[0.0; 4]; 4],
    ///     mirrored: false,
    ///     name: None,
    ///     extras: Some(r#"{"floats": true, "solid": false}"#.to_owned()),
    /// };
    /// assert!(pad.floats() && !pad.is_water());
    /// ```
    #[must_use]
    pub fn floats(&self) -> bool {
        self.ticked("floats").unwrap_or(false)
    }

    /// How far the wind moves it: 0 for a thing that stands still, which is
    /// everything unless a level's maker says otherwise, and 1 for a tree
    /// or a plant that bends and flutters as real ones do. The custom
    /// property `wind` holds the number; more than 1 is a reed or a rope,
    /// less is something stiff.
    ///
    /// It bends from where it stands (the object's own origin), more the
    /// higher up, so the origin belongs at the foot of the trunk.
    ///
    /// ```
    /// use lore_assets::Instance;
    ///
    /// let fern = Instance {
    ///     mesh: 0,
    ///     transform: [[0.0; 4]; 4],
    ///     mirrored: false,
    ///     name: None,
    ///     extras: Some(r#"{"wind": 1.5, "solid": false}"#.to_owned()),
    /// };
    /// assert_eq!(fern.wind(), 1.5);
    /// ```
    #[must_use]
    pub fn wind(&self) -> f32 {
        use gltf::json::{Value, deserialize::from_str};

        let Some(Ok(Value::Object(properties))) = self.extras.as_deref().map(from_str::<Value>)
        else {
            return 0.0;
        };
        // The number is a setting, and no finer than an f32.
        #[allow(clippy::cast_possible_truncation)]
        let wind = match properties.get("wind") {
            Some(Value::Number(wind)) => wind.as_f64().unwrap_or(0.0) as f32,
            Some(Value::Bool(true)) => 1.0,
            _ => 0.0,
        };
        if wind.is_finite() {
            wind.clamp(0.0, 4.0)
        } else {
            0.0
        }
    }

    /// The kind of fluid a sheet of still water is, if its maker said: the
    /// custom property `fluid` holds the kind's name, such as `"lava"` or
    /// `"honey"` (a ready-made kind, or one the game's script makes with
    /// `game.fluid`). With none it is water.
    ///
    /// ```
    /// use lore_assets::Instance;
    ///
    /// let pit = Instance {
    ///     mesh: 0,
    ///     transform: [[0.0; 4]; 4],
    ///     mirrored: false,
    ///     name: None,
    ///     extras: Some(r#"{"water": true, "fluid": " Lava "}"#.to_owned()),
    /// };
    /// assert_eq!(pit.fluid().as_deref(), Some("lava"));
    /// ```
    #[must_use]
    pub fn fluid(&self) -> Option<String> {
        use gltf::json::{Value, deserialize::from_str};

        let Some(Ok(Value::Object(properties))) = self.extras.as_deref().map(from_str::<Value>)
        else {
            return None;
        };
        match properties.get("fluid")? {
            Value::String(name) => {
                let name = name.trim().to_lowercase();
                (!name.is_empty()).then_some(name)
            }
            _ => None,
        }
    }

    /// How full a sheet of still water starts, from 0 (an empty basin that
    /// can be filled) to 1 (full to where the sheet lies, which is what it
    /// is when its maker says nothing): the custom property `amount`.
    ///
    /// ```
    /// use lore_assets::Instance;
    ///
    /// let tub = Instance {
    ///     mesh: 0,
    ///     transform: [[0.0; 4]; 4],
    ///     mirrored: false,
    ///     name: None,
    ///     extras: Some(r#"{"water": true, "amount": 0.25}"#.to_owned()),
    /// };
    /// assert_eq!(tub.amount(), 0.25);
    /// ```
    #[must_use]
    pub fn amount(&self) -> f32 {
        use gltf::json::{Value, deserialize::from_str};

        let Some(Ok(Value::Object(properties))) = self.extras.as_deref().map(from_str::<Value>)
        else {
            return 1.0;
        };
        // The number is a setting, and no finer than an f32.
        #[allow(clippy::cast_possible_truncation)]
        let amount = match properties.get("amount") {
            Some(Value::Number(amount)) => amount.as_f64().unwrap_or(1.0) as f32,
            Some(Value::Bool(false)) => 0.0,
            _ => 1.0,
        };
        if amount.is_finite() {
            amount.clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// What the custom property `name` says, if it is there and is true,
    /// false or a number: Blender writes a ticked box as 1 and an unticked
    /// one as 0 when the property was made as a whole number.
    fn ticked(&self, name: &str) -> Option<bool> {
        use gltf::json::{Value, deserialize::from_str};

        let Some(Ok(Value::Object(properties))) = self.extras.as_deref().map(from_str::<Value>)
        else {
            return None;
        };
        match properties.get(name)? {
            Value::Bool(ticked) => Some(*ticked),
            Value::Number(ticked) => Some(ticked.as_f64() != Some(0.0)),
            _ => None,
        }
    }
}

/// A body of still water: a lake, a pond, a pool, the water in a trough.
///
/// A level's maker lays a sheet marked `water` anywhere over a hollow, at
/// the height the water should stand ([`Instance::is_water`]). The engine
/// lets it run from there: it spreads over everything lower than itself
/// that it can reach, and stops at whatever stands in its way. What it
/// covered is kept here as a grid of points seen from above, each with how
/// deep the water is over it.
#[derive(Debug, Clone, PartialEq)]
pub struct Water {
    /// The name of the sheet it was filled from.
    pub name: Option<String>,
    /// Which of the scene's instances are sheets laid over this water:
    /// the one it was filled from, and any other that lies on it. They are
    /// not drawn: the water is drawn in the shape it took.
    pub sources: Vec<u32>,
    /// How high the surface stands when it is still, in metres.
    pub level: f32,
    /// How high rain can raise it, in metres: a little under the lowest
    /// point of the rim of the hollow it fills, where more would run out,
    /// or as high as rain raises any water if that is lower. Never under
    /// [`level`](Self::level). See [`Water::risen`].
    pub brim: f32,
    /// The x and z of the grid's first point: the one with the least of both.
    pub origin: [f32; 2],
    /// How far apart the grid's points are along x and along z, in metres.
    pub cell: f32,
    /// How many points the grid has along x, and how many along z.
    pub size: [u32; 2],
    /// For each point, row by row (all of the first z, then the next): how
    /// deep the water is there, in metres. A point the water did not reach
    /// has a depth below nothing: how far above the surface the ground
    /// stands there, down to [`Water::DRY`] for a wall or anything higher.
    pub depths: Vec<f32>,
    /// The colour of the water itself, and in the fourth number how clear
    /// it is: 1 is clear water, less is murkier.
    pub colour: [f32; 4],
    /// The kind of fluid it is, by name, if its sheet said
    /// ([`Instance::fluid`]): `None` is water.
    pub fluid: Option<String>,
    /// How full it starts, from 0 (empty) to 1 (up to
    /// [`level`](Self::level)): see [`Instance::amount`].
    pub amount: f32,
    /// Where it runs out when it is filled past its
    /// [`brim`](Self::brim), if a way out of its hollow was found.
    pub spill: Option<Spill>,
}

/// Where a body of still fluid runs out of its hollow: the lowest point of
/// its rim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spill {
    /// A point just outside the rim and as high as it, in metres: what
    /// runs out falls from here.
    pub place: [f32; 3],
    /// Which way out of the hollow is there, along x and z, one unit long.
    pub way: [f32; 2],
    /// How wide the stretch of rim it runs out over is, in metres, with
    /// `place` at its middle: the length of a wall's level top, or next to
    /// nothing for a notch.
    pub wide: f32,
}

impl Water {
    /// The depth written down for a point the water is nowhere near
    /// reaching: a wall, a bank two metres high, the far side of a dam.
    pub const DRY: f32 = -2.0;

    /// The depth at one point of the grid: `column` along x and `row`
    /// along z. Outside the grid it is [`Water::DRY`].
    #[must_use]
    pub fn depth(&self, column: i64, row: i64) -> f32 {
        let [across, deep] = self.size;
        if column < 0 || row < 0 || column >= i64::from(across) || row >= i64::from(deep) {
            return Self::DRY;
        }
        // Both were just checked to be inside the grid, which fits a u32.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let at = row as usize * across as usize + column as usize;
        self.depths.get(at).copied().unwrap_or(Self::DRY)
    }

    /// How far rain that has raised still water `rise` metres raises this
    /// body: as far, but no higher than its [`brim`](Self::brim), where it
    /// runs out over its rim. Less than nothing, or what is no number, is
    /// nothing.
    ///
    /// ```
    /// use lore_assets::Water;
    ///
    /// // A pond whose rim is lowest a quarter of a metre over its water.
    /// let pond = Water {
    ///     name: None,
    ///     sources: vec![0],
    ///     level: 1.0,
    ///     brim: 1.25,
    ///     origin: [0.0, 0.0],
    ///     cell: 1.0,
    ///     size: [1, 1],
    ///     depths: vec![0.5],
    ///     colour: [0.0, 0.1, 0.1, 1.0],
    ///     fluid: None,
    ///     amount: 1.0,
    ///     spill: None,
    /// };
    /// assert_eq!(pond.risen(0.125), 0.125);
    /// assert_eq!(pond.risen(0.8), 0.25, "it runs out over the rim");
    /// assert_eq!(pond.risen(f32::NAN), 0.0);
    /// ```
    #[must_use]
    pub fn risen(&self, rise: f32) -> f32 {
        let room = (self.brim - self.level).max(0.0);
        if rise.is_finite() {
            rise.clamp(0.0, room)
        } else {
            0.0
        }
    }

    /// How far over where it was put (below nothing: under it) the surface
    /// stands when it is `shift` metres from there: no higher than its
    /// [`brim`](Self::brim), where it runs out, and no lower than its
    /// deepest point, where it is empty. What is no number is nothing.
    ///
    /// Unlike [`risen`](Self::risen) this goes down as well as up: a pool
    /// that is drained stands lower than its maker put it.
    #[must_use]
    pub fn stood(&self, shift: f32) -> f32 {
        let room = (self.brim - self.level).max(0.0);
        if shift.is_finite() {
            shift.clamp(-self.deepest(), room)
        } else {
            0.0
        }
    }

    /// How deep it is at its deepest point when it stands where it was
    /// put, in metres: nothing for a body with no water in it.
    #[must_use]
    pub fn deepest(&self) -> f32 {
        self.depths.iter().copied().fold(0.0, f32::max)
    }

    /// How deep the water is at `x`, `z`, in metres, read between the
    /// grid's points; below nothing where there is none.
    ///
    /// ```
    /// use lore_assets::Water;
    ///
    /// // A trough a metre long: two points across, dry at one end.
    /// let trough = Water {
    ///     name: None,
    ///     sources: vec![0],
    ///     level: 1.0,
    ///     brim: 1.2,
    ///     origin: [0.0, 0.0],
    ///     cell: 1.0,
    ///     size: [2, 1],
    ///     depths: vec![0.5, -0.5],
    ///     colour: [0.0, 0.1, 0.1, 1.0],
    ///     fluid: None,
    ///     amount: 1.0,
    ///     spill: None,
    /// };
    /// assert!((trough.depth_at(0.25, 0.0) - 0.25).abs() < 1e-6);
    /// assert!(trough.depth_at(0.75, 0.0) < 0.0);
    /// assert_eq!(trough.depth_at(-5.0, 0.0), Water::DRY);
    /// ```
    #[must_use]
    pub fn depth_at(&self, x: f32, z: f32) -> f32 {
        if !(self.cell > 0.0 && x.is_finite() && z.is_finite()) {
            return Self::DRY;
        }
        let along = (x - self.origin[0]) / self.cell;
        let down = (z - self.origin[1]) / self.cell;
        let [across, deep] = self.size;
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let (most_along, most_down) = (across as f32 - 1.0, deep as f32 - 1.0);
        if along < -0.5 || down < -0.5 || along > most_along + 0.5 || down > most_down + 0.5 {
            return Self::DRY;
        }
        let (along, down) = (along.clamp(0.0, most_along), down.clamp(0.0, most_down));
        // Both are inside the grid, which fits an i64 many times over.
        #[allow(clippy::cast_possible_truncation)]
        let (column, row) = (along.floor() as i64, down.floor() as i64);
        #[allow(clippy::cast_precision_loss)]
        let (share_x, share_z) = (along - column as f32, down - row as f32);
        let near =
            self.depth(column, row) * (1.0 - share_x) + self.depth(column + 1, row) * share_x;
        let far = self.depth(column, row + 1) * (1.0 - share_x)
            + self.depth(column + 1, row + 1) * share_x;
        // At the last column or row the share of the next is nothing, so
        // what is read past the grid's end does not count.
        near * (1.0 - share_z) + far * share_z
    }
}

/// A named place with no shape, such as a spawn point or an objective.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// The name given in the 3D tool.
    pub name: Option<String>,
    /// Where it stands and which way it faces.
    pub transform: Transform,
    /// The custom properties set in the 3D tool, as JSON text.
    pub extras: Option<String>,
}

/// The most joints one [`Skin`] may have.
///
/// A joint's number then fits in a byte and a skin's matrices in one small,
/// fixed-size block on the graphics card.
pub const MAX_JOINTS: usize = 256;

/// One object in the file's tree, kept so skins and animations can move it.
///
/// Parents come before their children in [`Scene::nodes`], so one pass from
/// the front works out where everything stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// The name given in the 3D tool. For a joint, this is the bone's name.
    pub name: Option<String>,
    /// The node this one hangs from, which is earlier in the list, or `None`
    /// at the top of the tree.
    pub parent: Option<u32>,
    /// Where it sits relative to its parent when nothing is animated.
    pub rest: Pose,
    /// The [`Instance`] this node places, if it has a mesh.
    pub instance: Option<u32>,
    /// The [`Skin`] that bends that instance. When set, the instance's points
    /// are placed by the skin's joints alone and its own `transform` is not used.
    pub skin: Option<u32>,
}

/// Where a node sits relative to its parent: scaled, then turned, then moved.
///
/// These are the three things an animation changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// How far it is moved, in metres.
    pub translation: [f32; 3],
    /// How it is turned: a unit quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    /// How much it is stretched along each axis.
    pub scale: [f32; 3],
}

impl Default for Pose {
    /// Not moved, turned or stretched.
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

impl Pose {
    /// The pose as a matrix.
    ///
    /// Only adds and multiplies, so every computer gets the same answer.
    #[must_use]
    pub fn transform(&self) -> Transform {
        let [x, y, z, w] = self.rotation;
        let [sx, sy, sz] = self.scale;
        let [tx, ty, tz] = self.translation;
        [
            [
                (1.0 - 2.0 * (y * y + z * z)) * sx,
                2.0 * (x * y + z * w) * sx,
                2.0 * (x * z - y * w) * sx,
                0.0,
            ],
            [
                2.0 * (x * y - z * w) * sy,
                (1.0 - 2.0 * (x * x + z * z)) * sy,
                2.0 * (y * z + x * w) * sy,
                0.0,
            ],
            [
                2.0 * (x * z + y * w) * sz,
                2.0 * (y * z - x * w) * sz,
                (1.0 - 2.0 * (x * x + y * y)) * sz,
                0.0,
            ],
            [tx, ty, tz, 1.0],
        ]
    }
}

/// The skeleton of a rigged mesh.
///
/// A point of the mesh follows up to four joints. Where joint `j` puts a
/// point is the joint node's place in the scene, times
/// `joints[j].inverse_bind`, times the point.
#[derive(Debug, Clone, PartialEq)]
pub struct Skin {
    /// The name given in the 3D tool (in Blender, the armature's).
    pub name: Option<String>,
    /// The joints, in the order [`MeshPart::joints`] numbers them.
    /// Between 1 and [`MAX_JOINTS`] of them.
    pub joints: Vec<Joint>,
}

/// One joint (bone) of a [`Skin`].
#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    /// Which of the scene's nodes is this joint.
    pub node: u32,
    /// Takes a point of the mesh to where it sits relative to this joint
    /// in the pose the mesh was rigged in.
    pub inverse_bind: Transform,
}

/// One animation clip, such as a walk cycle or a reload.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    /// The name given in the 3D tool (in Blender, the action's).
    pub name: Option<String>,
    /// How long it lasts in seconds: the time of its last key.
    pub duration: f32,
    /// What it moves. A node's translation, rotation and scale each have at
    /// most one track; whatever has no track stays at the node's rest pose.
    pub tracks: Vec<Track>,
}

/// How one node's translation, rotation or scale changes during an [`Animation`].
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Which of the scene's nodes it moves.
    pub node: u32,
    /// How to fill in the time between keys.
    pub interpolation: Interpolation,
    /// When each key is, in seconds from the start of the clip. There is at
    /// least one, none is negative and each is later than the one before.
    pub times: Vec<f32>,
    /// The value at each key.
    pub keys: Keys,
}

/// The keys of a [`Track`].
///
/// There is one value per time, except with [`Interpolation::CubicSpline`],
/// where there are three: the slope coming in, the value, and the slope going out.
#[derive(Debug, Clone, PartialEq)]
pub enum Keys {
    /// Where the node is moved to, as [`Pose::translation`].
    Translation(Vec<[f32; 3]>),
    /// How the node is turned, as [`Pose::rotation`]. Values are unit
    /// quaternions; the slopes of a cubic spline are not.
    Rotation(Vec<[f32; 4]>),
    /// How the node is stretched, as [`Pose::scale`].
    Scale(Vec<[f32; 3]>),
}

/// How a [`Track`] fills in the time between two keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Holds each key's value until the next key.
    Step,
    /// Moves in a straight line from each key to the next.
    Linear,
    /// Follows a smooth curve through the keys (glTF's cubic Hermite spline),
    /// shaped by the slopes stored either side of each value.
    CubicSpline,
}

/// A light placed in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    /// The name given in the 3D tool.
    pub name: Option<String>,
    /// What sort of light it is.
    pub kind: LightKind,
    /// Its colour (red, green, blue; linear).
    pub color: [f32; 3],
    /// How bright it is: lux for a sun, candela for the others.
    pub intensity: f32,
    /// How far it reaches in metres, if the file sets a limit.
    pub range: Option<f32>,
    /// Where it is. Means nothing for a sun.
    pub position: [f32; 3],
    /// Which way it shines. Means nothing for a point light.
    pub direction: [f32; 3],
}

/// The sorts of light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LightKind {
    /// Shines the same way everywhere, like the sun.
    Sun,
    /// Shines in every direction from one point, like a bulb.
    Point,
    /// Shines in a cone, like a torch.
    Spot {
        /// The angle from the middle of the cone to where it starts to fade, in radians.
        inner_angle: f32,
        /// The angle from the middle of the cone to where it ends, in radians.
        outer_angle: f32,
    },
}

/// How a surface looks: glTF's metallic-roughness material.
///
/// The base colour and emissive textures hold sRGB colours; the others hold
/// plain numbers and must not be colour-converted.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// The name given in the 3D tool.
    pub name: Option<String>,
    /// The surface colour (red, green, blue, alpha; linear), multiplied with the texture.
    pub base_color: [f32; 4],
    /// The surface colour picture.
    pub base_color_texture: Option<TextureRef>,
    /// 0 for plastic, stone and wood; 1 for bare metal.
    pub metallic: f32,
    /// 0 for a mirror finish, 1 for fully matte.
    pub roughness: f32,
    /// Roughness in the green channel and metallic in the blue, multiplied
    /// with the two numbers above.
    pub metallic_roughness_texture: Option<TextureRef>,
    /// Fine surface detail as a normal map.
    pub normal_texture: Option<TextureRef>,
    /// How strongly the normal map bends the surface.
    pub normal_scale: f32,
    /// Baked shadowing in crevices, in the red channel.
    pub occlusion_texture: Option<TextureRef>,
    /// How strongly the occlusion texture darkens, from 0 to 1.
    pub occlusion_strength: f32,
    /// The light the surface gives off (red, green, blue; linear),
    /// multiplied with the texture and with `emissive_strength`.
    pub emissive: [f32; 3],
    /// The picture of the light the surface gives off.
    pub emissive_texture: Option<TextureRef>,
    /// Brightens `emissive` beyond 1 for surfaces that glow.
    pub emissive_strength: f32,
    /// What the alpha of the base colour does.
    pub alpha_mode: AlphaMode,
    /// The surface is drawn from behind as well.
    pub double_sided: bool,
    /// The custom properties set in the 3D tool, as JSON text.
    pub extras: Option<String>,
}

impl Material {
    /// How the material's pictures may be moved to hide that they repeat,
    /// if its maker asked for that with the custom property `shuffle`.
    ///
    /// A wall sixty metres long is covered by one photograph of two or
    /// three metres of brick, and the same few bricks come round in a
    /// grid. A material with `shuffle` is drawn blended with itself moved
    /// elsewhere, differently from patch to patch, and a little lighter
    /// and darker in patches, so that no grid is seen.
    ///
    /// The two numbers say how far a picture can be moved without breaking
    /// its pattern: how many equal steps fit across one copy of it, and how
    /// many up. For brickwork they are the bricks along a course and the
    /// courses (or half the courses, where every other course is set half a
    /// brick along). 0 is for a direction with no pattern, where any move
    /// will do. `"shuffle": true` is `[0, 0]`: asphalt, plaster, concrete.
    ///
    /// ```
    /// use lore_assets::Material;
    ///
    /// let bricks = Material {
    ///     extras: Some(r#"{"shuffle": [8, 11]}"#.to_owned()),
    ///     ..Material::default()
    /// };
    /// assert_eq!(bricks.shuffle(), Some([8, 11]));
    ///
    /// let asphalt = Material {
    ///     extras: Some(r#"{"shuffle": true}"#.to_owned()),
    ///     ..Material::default()
    /// };
    /// assert_eq!(asphalt.shuffle(), Some([0, 0]));
    /// assert_eq!(Material::default().shuffle(), None);
    /// ```
    #[must_use]
    pub fn shuffle(&self) -> Option<[u32; 2]> {
        use gltf::json::{Value, deserialize::from_str};

        /// No picture has more steps across it than this: more is a mistake
        /// in the level, and is taken as no pattern at all.
        const MOST_STEPS: f64 = 4096.0;

        let Some(Ok(Value::Object(properties))) = self.extras.as_deref().map(from_str::<Value>)
        else {
            return None;
        };
        // A whole number of steps from 1 to the most there can be; anything
        // else is no pattern that way.
        let steps = |value: &Value| match value.as_f64() {
            // It was just checked to be a small whole number.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Some(steps) if (1.0..=MOST_STEPS).contains(&steps) && steps.fract() == 0.0 => {
                steps as u32
            }
            _ => 0,
        };
        match properties.get("shuffle")? {
            Value::Bool(true) => Some([0, 0]),
            Value::Number(on) if on.as_f64() != Some(0.0) => Some([0, 0]),
            Value::Array(moves) => match moves.as_slice() {
                [across, up] => Some([steps(across), steps(up)]),
                _ => Some([0, 0]),
            },
            _ => None,
        }
    }
}

/// What a material says of the other material painted over it: see
/// [`Material::paint`].
#[derive(Debug, Clone, PartialEq)]
pub struct Paint {
    /// The name of the material that is painted over this one.
    pub with: String,
    /// How soft the edge between the two is, from 0 (a knife's edge) to 1
    /// (a slow fade). It is 0.15 unless the material says.
    pub edge: f32,
    /// How ragged the edge is, from 0 (it follows the mesh's own fade from
    /// point to point, straight and even) to 1 (broken into islands and
    /// inlets, as plaster comes away from brick). It is 0.8 unless the
    /// material says.
    pub ragged: f32,
}

impl Material {
    /// The other material painted over this one, if its maker asked for one
    /// with the custom property `paint`.
    ///
    /// A real wall is not one material with a rectangle of another stuck on
    /// it: plaster has come away from brick in patches, and a road is
    /// asphalt with tar patches. A material with `paint` shows the other
    /// material wherever the alpha of the mesh's colour attribute is rubbed
    /// out (0 is all the other material, 1 none of it, which is what a mesh
    /// that was never painted has). All of the other material is used: its
    /// colour, bumps, roughness and shading. The edge between the two is
    /// broken up by fine noise, so it is ragged and has no seam.
    ///
    /// `paint` is the other material's name, or `with` its name and how
    /// `edge` (soft) and `ragged` the edge is, each from 0 to 1.
    ///
    /// ```
    /// use lore_assets::Material;
    ///
    /// let brick = Material {
    ///     extras: Some(r#"{"paint": "old plaster"}"#.to_owned()),
    ///     ..Material::default()
    /// };
    /// let paint = brick.paint().unwrap();
    /// assert_eq!(paint.with, "old plaster");
    /// assert_eq!((paint.edge, paint.ragged), (0.15, 0.8));
    ///
    /// let road = Material {
    ///     extras: Some(r#"{"paint": {"with": "tar", "edge": 0.05, "ragged": 1}}"#.to_owned()),
    ///     ..Material::default()
    /// };
    /// assert_eq!(road.paint().unwrap().edge, 0.05);
    /// assert_eq!(Material::default().paint(), None);
    /// ```
    #[must_use]
    pub fn paint(&self) -> Option<Paint> {
        use gltf::json::{Value, deserialize::from_str};

        const EDGE: f32 = 0.15;
        const RAGGED: f32 = 0.8;

        let Some(Ok(Value::Object(properties))) = self.extras.as_deref().map(from_str::<Value>)
        else {
            return None;
        };
        // A share from 0 to 1, or `usual` for anything else.
        let share = |value: Option<&Value>, usual: f32| match value.and_then(Value::as_f64) {
            // A setting, and no finer than an f32.
            #[allow(clippy::cast_possible_truncation)]
            Some(said) if said.is_finite() => (said as f32).clamp(0.0, 1.0),
            _ => usual,
        };
        match properties.get("paint")? {
            Value::String(with) if !with.trim().is_empty() => Some(Paint {
                with: with.trim().to_owned(),
                edge: EDGE,
                ragged: RAGGED,
            }),
            Value::Object(said) => match said.get("with") {
                Some(Value::String(with)) if !with.trim().is_empty() => Some(Paint {
                    with: with.trim().to_owned(),
                    edge: share(said.get("edge"), EDGE),
                    ragged: share(said.get("ragged"), RAGGED),
                }),
                _ => None,
            },
            _ => None,
        }
    }
}

impl Default for Material {
    /// Matte white: what a shape with no material is drawn with.
    fn default() -> Self {
        Self {
            name: None,
            base_color: [1.0; 4],
            base_color_texture: None,
            metallic: 0.0,
            roughness: 1.0,
            metallic_roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            occlusion_texture: None,
            occlusion_strength: 1.0,
            emissive: [0.0; 3],
            emissive_texture: None,
            emissive_strength: 1.0,
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
            extras: None,
        }
    }
}

/// What the alpha of a material's base colour does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlphaMode {
    /// Nothing: the surface is solid.
    Opaque,
    /// Cuts holes: anything with alpha below `cutoff` is not drawn.
    Mask {
        /// The alpha below which the surface is not drawn.
        cutoff: f32,
    },
    /// Makes the surface see-through.
    Blend,
}

/// A material's use of one texture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureRef {
    /// Which of the scene's textures is used.
    pub texture: u32,
    /// Which placement to use: 0 for a part's `uv0`, 1 for `uv1`.
    pub uv_set: u8,
    /// Slides, turns or repeats the texture, if the file asks for it.
    pub transform: Option<TextureTransform>,
}

/// A change made to texture coordinates before the texture is read:
/// scaled, then turned, then moved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureTransform {
    /// How far to move.
    pub offset: [f32; 2],
    /// How far to turn anticlockwise, in radians.
    pub rotation: f32,
    /// How many times to repeat.
    pub scale: [f32; 2],
}

/// A picture together with how to sample it.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// The name given in the 3D tool.
    pub name: Option<String>,
    /// Which of the scene's images is used.
    pub image: u32,
    /// How the picture repeats and is smoothed.
    pub sampler: Sampler,
}

/// A decoded picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// The name given in the 3D tool, or its file name.
    pub name: Option<String>,
    /// How many pixels wide it is.
    pub width: u32,
    /// How many pixels tall it is.
    pub height: u32,
    /// Four bytes per pixel (red, green, blue, alpha), row by row from the top.
    pub pixels: Vec<u8>,
}

/// How a texture repeats and is smoothed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sampler {
    /// What happens past the left and right edges.
    pub wrap_u: Wrap,
    /// What happens past the top and bottom edges.
    pub wrap_v: Wrap,
    /// How the texture looks when shown larger than it is.
    pub magnify: Filter,
    /// How the texture looks when shown smaller than it is.
    pub minify: Filter,
    /// How smaller copies of the texture are mixed, or `None` to not use them.
    pub mipmaps: Option<Filter>,
}

impl Default for Sampler {
    /// Repeats, and is smoothed in every way.
    fn default() -> Self {
        Self {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Repeat,
            magnify: Filter::Linear,
            minify: Filter::Linear,
            mipmaps: Some(Filter::Linear),
        }
    }
}

/// What a texture shows past its edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    /// The texture again.
    Repeat,
    /// The texture again, flipped each time.
    MirroredRepeat,
    /// The colour at the edge.
    ClampToEdge,
}

/// How neighbouring pixels are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// The nearest pixel is used: sharp and blocky.
    Nearest,
    /// Neighbours are blended: smooth.
    Linear,
}
