//! A model's tree of bones, and turning a pose of it into places.

use lore_assets::{MAX_JOINTS, Pose, Scene, Skin, Transform};

use crate::math::{IDENTITY, combine};

/// A model's tree of objects, ready to pose.
///
/// Every node of the file is a bone here, whether or not a skin uses it, so a
/// clip may move a prop or a camera hung on the rig as well. Parents come
/// before their children, so one pass from the front places them all.
#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton {
    names: Vec<Option<String>>,
    parents: Vec<Option<u32>>,
    rest: Vec<Pose>,
    skins: Vec<Skin>,
}

impl Skeleton {
    /// The skeleton of a loaded file: all its nodes and all its skins.
    #[must_use]
    pub fn from_scene(scene: &Scene) -> Self {
        Self {
            names: scene.nodes.iter().map(|node| node.name.clone()).collect(),
            parents: scene.nodes.iter().map(|node| node.parent).collect(),
            rest: scene.nodes.iter().map(|node| node.rest).collect(),
            skins: scene.skins.clone(),
        }
    }

    /// How many bones (nodes) it has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rest.len()
    }

    /// Whether it has no bones at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }

    /// Each bone's name, in order.
    pub fn names(&self) -> impl Iterator<Item = Option<&str>> {
        self.names.iter().map(Option::as_deref)
    }

    /// The number of the first bone called `name`.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n.as_deref() == Some(name))
    }

    /// The bone `bone` hangs from.
    #[must_use]
    pub fn parent(&self, bone: usize) -> Option<usize> {
        self.parents
            .get(bone)
            .copied()
            .flatten()
            .map(|p| p as usize)
    }

    /// Every bone at rest: the pose the model was made in.
    #[must_use]
    pub fn rest(&self) -> &[Pose] {
        &self.rest
    }

    /// The file's skins (the skeletons that bend its meshes).
    #[must_use]
    pub fn skins(&self) -> &[Skin] {
        &self.skins
    }

    /// Where each bone stands relative to the model, from each bone's pose
    /// relative to its parent.
    ///
    /// `local` and `out` are as long as the skeleton; anything shorter is
    /// placed as far as it goes. Nothing is allocated.
    pub fn place(&self, local: &[Pose], out: &mut [Transform]) {
        let count = self.parents.len().min(local.len()).min(out.len());
        for bone in 0..count {
            let own = local[bone].transform();
            out[bone] = match self.parents[bone] {
                // Parents come first, so the parent's place is already known.
                Some(parent) if (parent as usize) < bone => combine(&out[parent as usize], &own),
                _ => own,
            };
        }
    }

    /// The matrices that bend skin `skin`'s points: each joint's place times
    /// its inverse bind matrix. These go to the graphics card.
    ///
    /// `placed` is what [`Skeleton::place`] gave. `out` holds at least as
    /// many matrices as the skin has joints (never more than [`MAX_JOINTS`]);
    /// a joint with no such skin is left as it is. Returns how many were written.
    pub fn skinning(&self, skin: usize, placed: &[Transform], out: &mut [Transform]) -> usize {
        let Some(skin) = self.skins.get(skin) else {
            return 0;
        };
        let mut written = 0;
        for (joint, slot) in skin.joints.iter().zip(out.iter_mut()).take(MAX_JOINTS) {
            let place = placed.get(joint.node as usize).unwrap_or(&IDENTITY);
            *slot = combine(place, &joint.inverse_bind);
            written += 1;
        }
        written
    }
}

/// The bones of LORE's standard skeleton (Unreal's mannequin names, which
/// LORE's own bodies use), leaving out the fingers. A model with these
/// plays every clip made for the mannequin.
pub const STANDARD_BONES: [&str; 23] = [
    "root",
    "pelvis",
    "spine_01",
    "spine_02",
    "spine_03",
    "neck_01",
    "Head",
    "clavicle_l",
    "upperarm_l",
    "lowerarm_l",
    "hand_l",
    "clavicle_r",
    "upperarm_r",
    "lowerarm_r",
    "hand_r",
    "thigh_l",
    "calf_l",
    "foot_l",
    "ball_l",
    "thigh_r",
    "calf_r",
    "foot_r",
    "ball_r",
];

impl Skeleton {
    /// The bones of [`STANDARD_BONES`] this skeleton does not have, each
    /// with the name of one of its own that looks like it, if any: what to
    /// rename so the mannequin's clips play on it.
    #[must_use]
    pub fn missing_standard_bones(&self) -> Vec<(&'static str, Option<String>)> {
        STANDARD_BONES
            .iter()
            .filter(|bone| self.find(bone).is_none())
            .map(|&bone| {
                let wanted = simplified(bone);
                let near = self
                    .names()
                    .flatten()
                    .find(|own| simplified(own) == wanted)
                    .map(str::to_owned);
                (bone, near)
            })
            .collect()
    }
}

/// A bone name with case, separators and common prefixes taken away, so
/// `Thigh_L`, `thigh.l` and `mixamorig:thigh_l` all read `thighl`.
fn simplified(name: &str) -> String {
    let name = name.rsplit(':').next().unwrap_or(name);
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}
