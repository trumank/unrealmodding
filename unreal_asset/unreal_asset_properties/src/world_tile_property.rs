//! World tile properties

use unreal_asset_base::types::vector::Vector;

use crate::property_prelude::*;
use crate::vector_property::{BoxProperty, IntPointProperty};

//todo: what is this file even doing in properties?
/// World tile layer
#[derive(FNameContainer, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FWorldTileLayer {
    /// Name
    pub name: Option<String>,
    /// Reserved
    pub reserved_0: i32,
    /// Reserved
    pub reserved_1: IntPointProperty,
    /// Streaming distance
    pub streaming_distance: Option<i32>,
    /// Is distance streaming enabled
    pub distance_streaming_enabled: Option<bool>,
}

impl FWorldTileLayer {
    /// Read an `FWorldTileLayer` from an asset
    pub fn new<Reader: ArchiveReader>(asset: &mut Reader) -> Result<Self, Error> {
        let object_version = asset.get_object_version();

        let new_ancestry = Ancestry::new(asset.get_parent_class_export_name().unwrap_or_default());
        let name = asset.read_fstring()?;
        let reserved_0 = asset.read_i32::<LE>()?;
        let reserved_1 = IntPointProperty::new(asset, FName::default(), new_ancestry, false, 0)?;

        let streaming_distance =
            match object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO_UPDATED {
                true => Some(asset.read_i32::<LE>()?),
                false => None,
            };

        let distance_streaming_enabled =
            match object_version >= ObjectVersion::VER_UE4_WORLD_LAYER_ENABLE_DISTANCE_STREAMING {
                true => Some(asset.read_i32::<LE>()? == 1),
                false => None,
            };

        Ok(FWorldTileLayer {
            name,
            reserved_0,
            reserved_1,
            streaming_distance,
            distance_streaming_enabled,
        })
    }

    /// Write an `FWorldTileLayer` to an asset
    pub fn write<Writer: ArchiveWriter>(&self, asset: &mut Writer) -> Result<(), Error> {
        let object_version = asset.get_object_version();

        asset.write_fstring(self.name.as_deref())?;
        asset.write_i32::<LE>(self.reserved_0)?;
        self.reserved_1.write(asset, false)?;

        if object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO_UPDATED {
            asset.write_i32::<LE>(
                self.streaming_distance
                    .ok_or_else(|| Error::no_data("object_version >= VER_UE4_WORLD_LEVEL_INFO_UPDATED but streaming_distance is None".to_string()))?,
            )?;
        }

        if object_version >= ObjectVersion::VER_UE4_WORLD_LAYER_ENABLE_DISTANCE_STREAMING {
            asset.write_i32::<LE>(
                match self.distance_streaming_enabled.ok_or_else(|| {
                    Error::no_data(
                        "object_version >= VER_UE4_WORLD_LAYER_ENABLE_DISTANCE_STREAMING but distance_streaming_enabled is None".to_string(),
                    )
                })? {
                    true => 1,
                    false => 0,
                },
            )?;
        }

        Ok(())
    }
}

/// World tile lod info
#[derive(FNameContainer, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct FWorldTileLODInfo {
    /// Relative streaming distance
    pub relative_streaming_distance: i32,
    /// Reserved
    pub reserved_0: OrderedFloat<f32>,
    /// Reserved
    pub reserved_1: OrderedFloat<f32>,
    /// Reserved
    pub reserved_2: i32,
    /// Reserved
    pub reserved_3: i32,
}

impl FWorldTileLODInfo {
    /// Read `FWorldTileLODInfo` from an asset
    pub fn new<Reader: ArchiveReader>(asset: &mut Reader) -> Result<Self, Error> {
        Ok(FWorldTileLODInfo {
            relative_streaming_distance: asset.read_i32::<LE>()?,
            reserved_0: OrderedFloat(asset.read_f32::<LE>()?),
            reserved_1: OrderedFloat(asset.read_f32::<LE>()?),
            reserved_2: asset.read_i32::<LE>()?,
            reserved_3: asset.read_i32::<LE>()?,
        })
    }

    /// Write `FWorldTileLODInfo` to an asset
    pub fn write<Writer: ArchiveWriter>(&self, asset: &mut Writer) -> Result<(), Error> {
        asset.write_i32::<LE>(self.relative_streaming_distance)?;
        asset.write_f32::<LE>(self.reserved_0.0)?;
        asset.write_f32::<LE>(self.reserved_1.0)?;
        asset.write_i32::<LE>(self.reserved_2)?;
        asset.write_i32::<LE>(self.reserved_3)?;
        Ok(())
    }
}

/// World tile ifno
#[derive(FNameContainer, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FWorldTileInfo {
    /// Position
    #[container_ignore]
    pub position: Vector<i32>,
    /// Bounds
    pub bounds: BoxProperty,
    //absolute_position: Vector<i32>, // not set in most recent version of uassetapi?
    /// Tile layer
    pub layer: FWorldTileLayer,
    /// Should hide in tile view
    pub hide_in_tile_view: Option<bool>,
    /// Parent tile package name
    pub parent_tile_package_name: Option<String>,
    /// Lod list
    pub lod_list: Option<Vec<FWorldTileLODInfo>>,
    /// Z-Order
    pub z_order: Option<i32>,
}

impl FWorldTileInfo {
    /// Read `FWorldTileInfo` from an asset
    pub fn new<Reader: ArchiveReader>(asset: &mut Reader) -> Result<Self, Error> {
        let version = asset.get_custom_version::<FFortniteMainBranchObjectVersion>();
        let object_version = asset.get_object_version();

        let position = match version.version
            < FFortniteMainBranchObjectVersion::WorldCompositionTile3DOffset as i32
        {
            true => Vector::new(asset.read_i32::<LE>()?, asset.read_i32::<LE>()?, 0),
            false => Vector::new(
                asset.read_i32::<LE>()?,
                asset.read_i32::<LE>()?,
                asset.read_i32::<LE>()?,
            ),
        };

        let new_ancestry = Ancestry::new(asset.get_parent_class_export_name().unwrap_or_default());
        let bounds = BoxProperty::new(asset, FName::default(), new_ancestry, false, 0)?;
        let layer = FWorldTileLayer::new(asset)?;

        let mut hide_in_tile_view = None;
        let mut parent_tile_package_name = None;
        if object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO_UPDATED {
            hide_in_tile_view = Some(asset.read_i32::<LE>()? == 1);
            parent_tile_package_name = asset.read_fstring()?;
        }

        let mut lod_list = None;
        if object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO_LOD_LIST {
            let num_entries = asset.read_i32::<LE>()? as usize;
            let mut list = Vec::with_capacity(num_entries);
            for _i in 0..num_entries {
                list.push(FWorldTileLODInfo::new(asset)?);
            }
            lod_list = Some(list);
        }

        let z_order = match object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO_ZORDER {
            true => Some(asset.read_i32::<LE>()?),
            false => None,
        };

        Ok(FWorldTileInfo {
            position,
            bounds,
            layer,
            hide_in_tile_view,
            parent_tile_package_name,
            lod_list,
            z_order,
        })
    }

    /// Write `FWorldTileInfo` to an asset
    pub fn write<Writer: ArchiveWriter>(&self, asset: &mut Writer) -> Result<(), Error> {
        let object_version = asset.get_object_version();

        if asset
            .get_custom_version::<FFortniteMainBranchObjectVersion>()
            .version
            < FFortniteMainBranchObjectVersion::WorldCompositionTile3DOffset as i32
        {
            asset.write_i32::<LE>(self.position.x)?;
            asset.write_i32::<LE>(self.position.y)?;
        } else {
            asset.write_i32::<LE>(self.position.x)?;
            asset.write_i32::<LE>(self.position.y)?;
            asset.write_i32::<LE>(self.position.z)?;
        }

        self.bounds.write(asset, false)?;
        self.layer.write(asset)?;

        if object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO_UPDATED {
            asset.write_i32::<LE>(
                match self
                    .hide_in_tile_view
                    .ok_or_else(|| Error::no_data("object_version >= VER_UE4_WORLD_LEVEL_INFO_UPDATED but hide_in_tile_view is None".to_string()))?
                {
                    true => 1,
                    false => 0,
                },
            )?;

            asset.write_fstring(self.parent_tile_package_name.as_deref())?;
        }

        if object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO_LOD_LIST {
            let lod_list = self.lod_list.as_ref().ok_or_else(|| {
                Error::no_data(
                    "object_version >= VER_UE4_WORLD_LEVEL_INFO_LOD_LIST but lod_list is None"
                        .to_string(),
                )
            })?;

            asset.write_i32::<LE>(lod_list.len() as i32)?;
            for entry in lod_list {
                entry.write(asset)?;
            }
        }

        if object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO_ZORDER {
            asset.write_i32::<LE>(self.z_order.ok_or_else(|| {
                Error::no_data(
                    "object_version >= VER_UE4_WORLD_LEVEL_INFO_ZORDER but z_order is None"
                        .to_string(),
                )
            })?)?;
        }

        Ok(())
    }
}
