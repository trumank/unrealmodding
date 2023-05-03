#![deny(missing_docs)]
//! This crate is used for parsing Unreal Engine uasset files
//!
//! # Examples
//!
//! ## Reading an asset that doesn't use bulk data
//!
//! ```no_run
//! use std::fs::File;
//!
//! use unreal_asset::{
//!     Asset,
//!     engine_version::EngineVersion,
//! };
//!
//! let mut file = File::open("asset.uasset").unwrap();
//! let mut asset = Asset::new(file, None, EngineVersion::VER_UE4_23).unwrap();
//!
//! println!("{:#?}", asset);
//! ```
//!
//! ## Reading an asset that uses bulk data
//!
//! ```no_run
//! use std::fs::File;
//!
//! use unreal_asset::{
//!     Asset,
//!     engine_version::EngineVersion,
//! };
//!
//! let mut file = File::open("asset.uasset").unwrap();
//! let mut bulk_file = File::open("asset.uexp").unwrap();
//! let mut asset = Asset::new(file, Some(bulk_file), EngineVersion::VER_UE4_23).unwrap();
//!
//! println!("{:#?}", asset);
//! ```
use std::fmt::{Debug, Formatter};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::size_of;

use asset::name_map::NameMap;
use asset::AssetData;
use byteorder::{BigEndian, LittleEndian};

use containers::shared_resource::SharedResource;
use engine_version::EngineVersion;
use enums::ECustomVersionSerializationFormat;
use reader::asset_archive_reader::AssetArchiveReader;
use reader::asset_archive_writer::AssetArchiveWriter;
use reader::raw_reader::RawReader;
use reader::raw_writer::RawWriter;
use unreal_asset_proc_macro::FNameContainer;

pub mod ac7;
pub mod asset;
pub mod containers;
mod crc;
pub mod custom_version;
pub mod engine_version;
pub mod enums;
pub mod error;
pub mod exports;
pub mod flags;
pub mod fproperty;
pub mod kismet;
pub mod object_version;
pub mod properties;
pub mod reader;
pub mod registry;
pub mod types;
pub mod unversioned;
pub mod uproperty;

use containers::chain::Chain;
use containers::indexed_map::IndexedMap;
use error::Error;
use exports::{
    base_export::BaseExport, class_export::ClassExport, data_table_export::DataTableExport,
    enum_export::EnumExport, function_export::FunctionExport, level_export::LevelExport,
    normal_export::NormalExport, property_export::PropertyExport, raw_export::RawExport,
    string_table_export::StringTableExport, Export, ExportBaseTrait, ExportNormalTrait,
    ExportTrait,
};
use flags::EPackageFlags;
use fproperty::FProperty;
use object_version::ObjectVersion;
use properties::world_tile_property::FWorldTileInfo;
use reader::{
    archive_reader::ArchiveReader, archive_trait::ArchiveTrait, archive_writer::ArchiveWriter,
};
use types::{
    fname::{FName, FNameContainer},
    GenerationInfo, Guid, PackageIndex,
};

/// Cast a Property/Export to a more specific type
///
/// # Examples
///
/// ```no_run,ignore
/// use unreal_asset::{
///     cast,
///     properties::{
///         Property,
///         int_property::DoubleProperty,
///     },
/// };
/// let a: Property = ...;
/// let b: &DoubleProperty = cast!(Property, DoubleProperty, &a).unwrap();
/// ```
#[macro_export]
macro_rules! cast {
    ($namespace:ident, $type:ident, $field:expr) => {
        match $field {
            $namespace::$type(e) => Some(e),
            _ => None,
        }
    };
}

/// Import struct for an Asset
///
/// This is used for referencing other assets
#[derive(FNameContainer, Debug, Clone, Eq, PartialEq)]
pub struct Import {
    /// Class package
    pub class_package: FName,
    /// Class name
    pub class_name: FName,
    /// Outer index
    #[container_ignore]
    pub outer_index: PackageIndex,
    /// Object name
    pub object_name: FName,
}

impl Import {
    /// Create a new `Import` instance
    pub fn new(
        class_package: FName,
        class_name: FName,
        outer_index: PackageIndex,
        object_name: FName,
    ) -> Self {
        Import {
            class_package,
            class_name,
            object_name,
            outer_index,
        }
    }
}

/// Parent Class Info
#[derive(FNameContainer, Debug, Clone, Eq, PartialEq)]
pub struct ParentClassInfo {
    /// Parent classpath
    pub parent_class_path: FName,
    /// Parent class export name
    pub parent_class_export_name: FName,
}

const UE4_ASSET_MAGIC: u32 = u32::from_be_bytes([0xc1, 0x83, 0x2a, 0x9e]);

/// Asset header
struct AssetHeader {
    /// Name map offset
    name_offset: i32,
    /// Imports offset
    import_offset: i32,
    /// Exports offset
    export_offset: i32,
    /// Dependencies offset
    depends_offset: i32,
    /// Soft package references offset
    soft_package_reference_offset: i32,
    /// Asset registry data offset
    asset_registry_data_offset: i32,
    /// World tile info offset
    world_tile_info_offset: i32,
    /// Preload dependency count
    preload_dependency_count: i32,
    /// Preload dependency offset
    preload_dependency_offset: i32,
    /// Header offset
    header_offset: i32,
    /// Bulk data start offset
    bulk_data_start_offset: i64,
}

//#[derive(Debug)]
/// Unreal Engine uasset
#[derive(FNameContainer)]
pub struct Asset {
    // parsed data
    /// Asset info
    pub info: String,
    /// Asset data
    pub asset_data: AssetData,
    /// Legacy file version
    pub legacy_file_version: i32,

    // imports
    // exports
    // depends map
    // soft package reference list
    // asset registry data
    // world tile info
    // preload dependencies
    /// Generations
    #[container_ignore]
    pub generations: Vec<GenerationInfo>,
    /// Asset guid
    pub package_guid: Guid,
    /// Recorded engine version
    #[container_ignore]
    pub engine_version_recorded: FEngineVersion,
    /// Compatible engine version
    #[container_ignore]
    pub engine_version_compatible: FEngineVersion,
    /// Chunk ids
    chunk_ids: Vec<i32>,
    /// Asset flags
    #[container_ignore]
    pub package_flags: EPackageFlags,
    /// Asset source
    pub package_source: u32,
    /// Folder name
    pub folder_name: String,

    // map struct type override
    // override name map hashes
    // todo: isn't this just AssetHeader?
    /// Header offset
    header_offset: i32,
    /// Name count
    name_count: i32,
    /// Name offset
    name_offset: i32,
    /// Gatherable text data count
    gatherable_text_data_count: i32,
    /// Gatherable text data offset
    gatherable_text_data_offset: i32,
    /// Export count
    export_count: i32,
    /// Exports offset
    export_offset: i32,
    /// Import count
    import_count: i32,
    /// Imports offset
    import_offset: i32,
    /// Depends offset
    depends_offset: i32,
    /// Soft package reference count
    soft_package_reference_count: i32,
    /// Soft package reference offset
    soft_package_reference_offset: i32,
    /// Searchable names offset
    searchable_names_offset: i32,
    /// Thumbnail table offset
    thumbnail_table_offset: i32,
    /// Compression flags
    compression_flags: u32,
    /// Asset registry data offset
    asset_registry_data_offset: i32,
    /// Bulk data start offset
    bulk_data_start_offset: i64,
    /// World tile info offset
    world_tile_info_offset: i32,
    /// Preload dependency count
    preload_dependency_count: i32,
    /// Preload dependency offset
    preload_dependency_offset: i32,

    /// Overriden name map hashes
    #[container_ignore]
    pub override_name_map_hashes: IndexedMap<String, u32>,
    /// Name map
    #[container_ignore]
    name_map: SharedResource<NameMap>,
    /// Imports
    pub imports: Vec<Import>,
    /// Depends map
    #[container_ignore]
    depends_map: Option<Vec<Vec<i32>>>,
    /// Soft package reference list
    #[container_ignore]
    soft_package_reference_list: Option<Vec<String>>,

    /// Parent class
    parent_class: Option<ParentClassInfo>,
}

/// Struct that stores new map/array key/value overrides
///
/// Returned from `read_export`
#[derive(Default)]
struct NewOverrides {
    /// New array overrides
    array_overrides: IndexedMap<String, String>,
    /// New map key overrides
    map_key_overrides: IndexedMap<String, String>,
    /// New map value overrides
    map_value_overrides: IndexedMap<String, String>,
}

impl NewOverrides {
    /// Apply overrides to asset data
    fn apply(self, asset_data: &mut AssetData) {
        asset_data
            .array_struct_type_override
            .extend(self.array_overrides.into_iter().map(|(_, k, v)| (k, v)));
        asset_data
            .map_key_override
            .extend(self.map_key_overrides.into_iter().map(|(_, k, v)| (k, v)));
        asset_data
            .map_value_override
            .extend(self.map_value_overrides.into_iter().map(|(_, k, v)| (k, v)));
    }
}

impl<'a> Asset {
    /// Create an asset from a binary file
    pub fn new<C: Read + Seek>(
        asset_data: C,
        bulk_data: Option<C>,
        engine_version: EngineVersion,
    ) -> Result<Self, Error> {
        let use_event_driven_loader = bulk_data.is_some();
        let mut asset = Asset {
            info: String::from("Serialized with unrealmodding/uasset"),
            asset_data: AssetData {
                use_event_driven_loader,
                ..Default::default()
            },
            legacy_file_version: 0,
            generations: Vec::new(),
            package_guid: [0; 16],
            engine_version_recorded: FEngineVersion::unknown(),
            engine_version_compatible: FEngineVersion::unknown(),
            chunk_ids: Vec::new(),
            package_flags: EPackageFlags::PKG_NONE,
            package_source: 0,
            folder_name: String::from(""),
            header_offset: 0,
            name_count: 0,
            name_offset: 0,
            gatherable_text_data_count: 0,
            gatherable_text_data_offset: 0,
            export_count: 0,
            export_offset: 0,
            import_count: 0,
            import_offset: 0,
            depends_offset: 0,
            soft_package_reference_count: 0,
            soft_package_reference_offset: 0,
            searchable_names_offset: 0,
            thumbnail_table_offset: 0,
            compression_flags: 0,
            asset_registry_data_offset: 0,
            bulk_data_start_offset: 0,
            world_tile_info_offset: 0,
            preload_dependency_count: 0,
            preload_dependency_offset: 0,

            override_name_map_hashes: IndexedMap::new(),
            name_map: NameMap::new(),
            imports: Vec::new(),
            depends_map: None,
            soft_package_reference_list: None,
            parent_class: None,
        };
        asset.set_engine_version(engine_version);

        let chain = Chain::new(asset_data, bulk_data);

        let mut reader = RawReader::new(
            chain,
            asset.asset_data.object_version,
            asset.asset_data.object_version_ue5,
            asset.asset_data.use_event_driven_loader,
            asset.name_map.clone(),
        );
        asset.parse_header(&mut reader)?;

        // updating reader objectversions because they might've been updated when reading the header
        reader.object_version = asset.asset_data.object_version;
        reader.object_version_ue5 = asset.asset_data.object_version_ue5;

        asset.parse_data(&mut reader)?;
        Ok(asset)
    }

    /// Set asset engine version
    fn set_engine_version(&mut self, engine_version: EngineVersion) {
        self.asset_data.set_engine_version(engine_version)
    }

    /// Parse asset header
    fn parse_header<R: ArchiveReader>(&mut self, reader: &mut R) -> Result<(), Error> {
        // reuseable buffers for reading

        // seek to start
        reader.seek(SeekFrom::Start(0))?;

        // read and check magic
        if reader.read_u32::<BigEndian>()? != UE4_ASSET_MAGIC {
            return Err(Error::invalid_file(
                "File is not a valid uasset file".to_string(),
            ));
        }

        // read legacy version
        self.legacy_file_version = reader.read_i32::<LittleEndian>()?;
        if self.legacy_file_version != -4 {
            // LegacyUE3Version for backwards-compatibility with UE3 games: always 864 in versioned assets, always 0 in unversioned assets
            reader.read_exact(&mut [0u8; 4])?;
        }

        // read unreal version
        let file_version = reader.read_i32::<LittleEndian>()?.try_into()?;

        self.asset_data.unversioned = file_version == ObjectVersion::UNKNOWN;

        if self.asset_data.unversioned {
            if self.asset_data.object_version == ObjectVersion::UNKNOWN {
                return Err(Error::invalid_file("Cannot begin serialization of an unversioned asset before an engine version is manually specified".to_string()));
            }
        } else {
            self.asset_data.object_version = file_version;
        }

        // read file license version
        self.asset_data.file_license_version = reader.read_i32::<LittleEndian>()?;

        // read custom versions container
        if self.legacy_file_version <= -2 {
            // TODO: support for enum-based custom versions
            let old_container = self.asset_data.custom_versions.clone();
            self.asset_data.custom_versions = reader.read_custom_version_container(
                self.get_custom_version_serialization_format(),
                Some(&old_container),
            )?;
        }

        // read header offset
        self.header_offset = reader.read_i32::<LittleEndian>()?;

        // read folder name
        self.folder_name = reader
            .read_fstring()?
            .ok_or_else(|| Error::no_data("folder_name is None".to_string()))?;

        // read package flags
        self.package_flags = EPackageFlags::from_bits(reader.read_u32::<LittleEndian>()?)
            .ok_or_else(|| Error::invalid_file("Invalid package flags".to_string()))?;

        // read name count and offset
        self.name_count = reader.read_i32::<LittleEndian>()?;
        self.name_offset = reader.read_i32::<LittleEndian>()?;
        // read text gatherable data
        if self.asset_data.object_version >= ObjectVersion::VER_UE4_SERIALIZE_TEXT_IN_PACKAGES {
            self.gatherable_text_data_count = reader.read_i32::<LittleEndian>()?;
            self.gatherable_text_data_offset = reader.read_i32::<LittleEndian>()?;
        }

        // read count and offset for exports, imports, depends, soft package references, searchable names, thumbnail table
        self.export_count = reader.read_i32::<LittleEndian>()?;
        self.export_offset = reader.read_i32::<LittleEndian>()?;
        self.import_count = reader.read_i32::<LittleEndian>()?;
        self.import_offset = reader.read_i32::<LittleEndian>()?;
        self.depends_offset = reader.read_i32::<LittleEndian>()?;
        if self.asset_data.object_version >= ObjectVersion::VER_UE4_ADD_STRING_ASSET_REFERENCES_MAP
        {
            self.soft_package_reference_count = reader.read_i32::<LittleEndian>()?;
            self.soft_package_reference_offset = reader.read_i32::<LittleEndian>()?;
        }
        if self.asset_data.object_version >= ObjectVersion::VER_UE4_ADDED_SEARCHABLE_NAMES {
            self.searchable_names_offset = reader.read_i32::<LittleEndian>()?;
        }
        self.thumbnail_table_offset = reader.read_i32::<LittleEndian>()?;

        // read guid
        reader.read_exact(&mut self.package_guid)?;

        // raed generations
        let generations_count = reader.read_i32::<LittleEndian>()?;
        for _ in 0..generations_count {
            let export_count = reader.read_i32::<LittleEndian>()?;
            let name_count = reader.read_i32::<LittleEndian>()?;
            self.generations.push(GenerationInfo {
                export_count,
                name_count,
            });
        }

        // read advanced engine version
        if self.asset_data.object_version >= ObjectVersion::VER_UE4_ENGINE_VERSION_OBJECT {
            self.engine_version_recorded = FEngineVersion::read(reader)?;
        } else {
            self.engine_version_recorded =
                FEngineVersion::new(4, 0, 0, reader.read_u32::<LittleEndian>()?, None);
        }
        if self.asset_data.object_version
            >= ObjectVersion::VER_UE4_PACKAGE_SUMMARY_HAS_COMPATIBLE_ENGINE_VERSION
        {
            self.engine_version_compatible = FEngineVersion::read(reader)?;
        } else {
            self.engine_version_compatible = self.engine_version_recorded.clone();
        }

        // read compression data
        self.compression_flags = reader.read_u32::<LittleEndian>()?;
        let compression_block_count = reader.read_u32::<LittleEndian>()?;
        if compression_block_count > 0 {
            return Err(Error::invalid_file(
                "Compression block count is not zero".to_string(),
            ));
        }

        self.package_source = reader.read_u32::<LittleEndian>()?;

        // some other old unsupported stuff
        let additional_to_cook = reader.read_i32::<LittleEndian>()?;
        if additional_to_cook != 0 {
            return Err(Error::invalid_file(
                "Additional to cook is not zero".to_string(),
            ));
        }
        if self.legacy_file_version > -7 {
            let texture_allocations_count = reader.read_i32::<LittleEndian>()?;
            if texture_allocations_count != 0 {
                return Err(Error::invalid_file(
                    "Texture allocations count is not zero".to_string(),
                ));
            }
        }

        self.asset_registry_data_offset = reader.read_i32::<LittleEndian>()?;
        self.bulk_data_start_offset = reader.read_i64::<LittleEndian>()?;

        if self.asset_data.object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO {
            self.world_tile_info_offset = reader.read_i32::<LittleEndian>()?;
        }

        if self.asset_data.object_version
            >= ObjectVersion::VER_UE4_CHANGED_CHUNKID_TO_BE_AN_ARRAY_OF_CHUNKIDS
        {
            let chunk_id_count = reader.read_i32::<LittleEndian>()?;

            for _ in 0..chunk_id_count {
                let chunk_id = reader.read_i32::<LittleEndian>()?;
                self.chunk_ids.push(chunk_id);
            }
        } else if self.asset_data.object_version
            >= ObjectVersion::VER_UE4_ADDED_CHUNKID_TO_ASSETDATA_AND_UPACKAGE
        {
            self.chunk_ids = vec![];
            self.chunk_ids[0] = reader.read_i32::<LittleEndian>()?;
        }

        if self.asset_data.object_version
            >= ObjectVersion::VER_UE4_PRELOAD_DEPENDENCIES_IN_COOKED_EXPORTS
        {
            self.preload_dependency_count = reader.read_i32::<LittleEndian>()?;
            self.preload_dependency_offset = reader.read_i32::<LittleEndian>()?;
        }
        Ok(())
    }

    /// Get name map
    /// This method should be used if you want to mutate the namemap
    ///
    /// # Panics
    ///
    /// If the name map is borrowed mutably and you try to write the asset, the lib will panic
    pub fn get_name_map(&self) -> SharedResource<NameMap> {
        self.name_map.clone()
    }

    /// Search an FName reference
    pub fn search_name_reference(&self, name: &str) -> Option<i32> {
        self.name_map.get_ref().search_name_reference(name)
    }

    /// Add an FName reference
    pub fn add_name_reference(&mut self, name: String, force_add_duplicates: bool) -> i32 {
        self.name_map
            .get_mut()
            .add_name_reference(name, force_add_duplicates)
    }

    /// Get a name reference by an FName map index
    pub fn get_name_reference(&self, index: i32) -> String {
        self.name_map.get_ref().get_name_reference(index)
    }

    /// Add an `FName`
    pub fn add_fname(&mut self, slice: &str) -> FName {
        self.name_map.get_mut().add_fname(slice)
    }

    /// Add an `Import`
    pub fn add_import(&mut self, import: Import) -> PackageIndex {
        let index = -(self.imports.len() as i32) - 1;
        let import = import;
        self.imports.push(import);
        PackageIndex::new(index)
    }

    /// Find an import
    pub fn find_import(
        &self,
        class_package: &FName,
        class_name: &FName,
        outer_index: PackageIndex,
        object_name: &FName,
    ) -> Option<i32> {
        for i in 0..self.imports.len() {
            let import = &self.imports[i];
            if import.class_package == *class_package
                && import.class_name == *class_name
                && import.outer_index == outer_index
                && import.object_name == *object_name
            {
                return Some(-(i as i32) - 1);
            }
        }
        None
    }

    /// Find an import without specifying outer index
    pub fn find_import_no_index(
        &self,
        class_package: &FName,
        class_name: &FName,
        object_name: &FName,
    ) -> Option<i32> {
        for i in 0..self.imports.len() {
            let import = &self.imports[i];
            if import.class_package == *class_package
                && import.class_name == *class_name
                && import.object_name == *object_name
            {
                return Some(-(i as i32) - 1);
            }
        }
        None
    }

    /// Get an export
    pub fn get_export(&'a self, index: PackageIndex) -> Option<&'a Export> {
        self.asset_data.get_export(index)
    }

    /// Get a mutable export reference
    pub fn get_export_mut(&'a mut self, index: PackageIndex) -> Option<&'a mut Export> {
        self.asset_data.get_export_mut(index)
    }

    /// Get custom version serialization format
    pub fn get_custom_version_serialization_format(&self) -> ECustomVersionSerializationFormat {
        if self.legacy_file_version > 3 {
            return ECustomVersionSerializationFormat::Enums;
        }
        if self.legacy_file_version > -6 {
            return ECustomVersionSerializationFormat::Guids;
        }
        ECustomVersionSerializationFormat::Optimized
    }

    /// Parse asset data
    fn parse_data<R: ArchiveReader>(&mut self, reader: &mut R) -> Result<(), Error> {
        reader.seek(SeekFrom::Start(self.name_offset as u64))?;

        for i in 0..self.name_count {
            println!("processing {}", i);
            let (name, hash) = reader.read_name_map_string(None)?;
            if hash == 0 {
                // todo: good FString type
                self.override_name_map_hashes.insert(name.clone(), 0);
            }
            self.add_name_reference(name, true);
        }

        if self.import_offset > 0 {
            reader.seek(SeekFrom::Start(self.import_offset as u64))?;
            for _i in 0..self.import_count {
                let import = Import::new(
                    reader.read_fname()?,
                    reader.read_fname()?,
                    PackageIndex::new(reader.read_i32::<LittleEndian>()?),
                    reader.read_fname()?,
                );
                self.imports.push(import);
            }
        }

        if self.export_offset > 0 {
            reader.seek(SeekFrom::Start(self.export_offset as u64))?;
            for _i in 0..self.export_count {
                let mut export = BaseExport {
                    class_index: PackageIndex::new(reader.read_i32::<LittleEndian>()?),
                    super_index: PackageIndex::new(reader.read_i32::<LittleEndian>()?),
                    ..Default::default()
                };

                if reader.get_object_version()
                    >= ObjectVersion::VER_UE4_TemplateIndex_IN_COOKED_EXPORTS
                {
                    export.template_index = PackageIndex::new(reader.read_i32::<LittleEndian>()?);
                }

                export.outer_index = PackageIndex::new(reader.read_i32::<LittleEndian>()?);
                export.object_name = reader.read_fname()?;
                export.object_flags = reader.read_u32::<LittleEndian>()?;

                if reader.get_object_version() < ObjectVersion::VER_UE4_64BIT_EXPORTMAP_SERIALSIZES
                {
                    export.serial_size = reader.read_i32::<LittleEndian>()? as i64;
                    export.serial_offset = reader.read_i32::<LittleEndian>()? as i64;
                } else {
                    export.serial_size = reader.read_i64::<LittleEndian>()?;
                    export.serial_offset = reader.read_i64::<LittleEndian>()?;
                }

                export.forced_export = reader.read_i32::<LittleEndian>()? == 1;
                export.not_for_client = reader.read_i32::<LittleEndian>()? == 1;
                export.not_for_server = reader.read_i32::<LittleEndian>()? == 1;
                reader.read_exact(&mut export.package_guid)?;
                export.package_flags = reader.read_u32::<LittleEndian>()?;

                if reader.get_object_version() >= ObjectVersion::VER_UE4_LOAD_FOR_EDITOR_GAME {
                    export.not_always_loaded_for_editor_game =
                        reader.read_i32::<LittleEndian>()? == 1;
                }

                if reader.get_object_version()
                    >= ObjectVersion::VER_UE4_COOKED_ASSETS_IN_EDITOR_SUPPORT
                {
                    export.is_asset = reader.read_i32::<LittleEndian>()? == 1;
                }

                if reader.get_object_version()
                    >= ObjectVersion::VER_UE4_PRELOAD_DEPENDENCIES_IN_COOKED_EXPORTS
                {
                    export.first_export_dependency_offset = reader.read_i32::<LittleEndian>()?;
                    export.serialization_before_serialization_dependencies_size =
                        reader.read_i32::<LittleEndian>()?;
                    export.create_before_serialization_dependencies_size =
                        reader.read_i32::<LittleEndian>()?;
                    export.serialization_before_create_dependencies_size =
                        reader.read_i32::<LittleEndian>()?;
                    export.create_before_create_dependencies_size =
                        reader.read_i32::<LittleEndian>()?;
                }

                self.asset_data.exports.push(export.into());
            }
        }

        if self.depends_offset > 0 {
            let mut depends_map = Vec::with_capacity(self.export_count as usize);

            reader.seek(SeekFrom::Start(self.depends_offset as u64))?;

            for _i in 0..self.export_count as usize {
                let size = reader.read_i32::<LittleEndian>()?;
                let mut data: Vec<i32> = Vec::new();
                for _j in 0..size {
                    data.push(reader.read_i32::<LittleEndian>()?);
                }
                depends_map.push(data);
            }
            self.depends_map = Some(depends_map);
        }

        if self.soft_package_reference_offset > 0 {
            let mut soft_package_reference_list =
                Vec::with_capacity(self.soft_package_reference_count as usize);

            reader.seek(SeekFrom::Start(self.soft_package_reference_offset as u64))?;

            for _i in 0..self.soft_package_reference_count as usize {
                if let Some(reference) = reader.read_fstring()? {
                    soft_package_reference_list.push(reference);
                }
            }
            self.soft_package_reference_list = Some(soft_package_reference_list);
        }

        // TODO: Asset registry data parsing should be here

        if self.world_tile_info_offset > 0 {
            reader.seek(SeekFrom::Start(self.world_tile_info_offset as u64))?;
            self.asset_data.world_tile_info = Some(FWorldTileInfo::new(reader)?);
        }

        if self.asset_data.use_event_driven_loader {
            for export in &mut self.asset_data.exports {
                let unk_export = export.get_base_export_mut();

                reader.seek(SeekFrom::Start(self.preload_dependency_offset as u64))?;
                reader.seek(SeekFrom::Current(
                    unk_export.first_export_dependency_offset as i64 * size_of::<i32>() as i64,
                ))?;

                let mut serialization_before_serialization_dependencies = Vec::with_capacity(
                    unk_export.serialization_before_serialization_dependencies_size as usize,
                );
                for _ in 0..unk_export.serialization_before_serialization_dependencies_size {
                    serialization_before_serialization_dependencies
                        .push(PackageIndex::new(reader.read_i32::<LittleEndian>()?));
                }
                unk_export.serialization_before_serialization_dependencies =
                    serialization_before_serialization_dependencies;

                let mut create_before_serialization_dependencies = Vec::with_capacity(
                    unk_export.create_before_serialization_dependencies_size as usize,
                );
                for _ in 0..unk_export.create_before_serialization_dependencies_size {
                    create_before_serialization_dependencies
                        .push(PackageIndex::new(reader.read_i32::<LittleEndian>()?));
                }
                unk_export.create_before_serialization_dependencies =
                    create_before_serialization_dependencies;

                let mut serialization_before_create_dependencies = Vec::with_capacity(
                    unk_export.serialization_before_create_dependencies_size as usize,
                );
                for _ in 0..unk_export.serialization_before_create_dependencies_size {
                    serialization_before_create_dependencies
                        .push(PackageIndex::new(reader.read_i32::<LittleEndian>()?));
                }
                unk_export.serialization_before_create_dependencies =
                    serialization_before_create_dependencies;

                let mut create_before_create_dependencies =
                    Vec::with_capacity(unk_export.create_before_create_dependencies_size as usize);
                for _ in 0..unk_export.create_before_create_dependencies_size {
                    create_before_create_dependencies
                        .push(PackageIndex::new(reader.read_i32::<LittleEndian>()?));
                }
                unk_export.create_before_create_dependencies = create_before_create_dependencies;
            }
            reader.seek(SeekFrom::Start(self.preload_dependency_offset as u64))?;
        }

        if self.header_offset > 0 && !self.asset_data.exports.is_empty() {
            let mut new_exports = Vec::with_capacity(self.asset_data.exports.len());
            for i in 0..self.asset_data.exports.len() {
                let mut asset_reader = AssetArchiveReader::new(
                    reader,
                    &self.asset_data,
                    &self.imports,
                    self.name_map.clone(),
                );

                let base_export = match &self.asset_data.exports[i] {
                    Export::BaseExport(export) => Some(export.clone()),
                    _ => None,
                };

                if let Some(base_export) = base_export {
                    let result = self.read_export(&mut asset_reader, &base_export, i);
                    let export: Result<(Export, NewOverrides), Error> = match result {
                        Ok(e) => Ok(e),
                        Err(_e) => {
                            // todo: warning?
                            asset_reader.seek(SeekFrom::Start(base_export.serial_offset as u64))?;
                            Ok((
                                RawExport::from_base(base_export, &mut asset_reader)?.into(),
                                NewOverrides::default(),
                            ))
                        }
                    };
                    let (export, new_overrides) = export?;

                    drop(asset_reader);
                    new_overrides.apply(&mut self.asset_data);

                    new_exports.push(export);
                }
            }

            self.asset_data.exports = new_exports;
        }

        Ok(())
    }

    /// Read an `Export`
    fn read_export<R: ArchiveReader>(
        &self,
        reader: &mut R,
        base_export: &BaseExport,
        i: usize,
    ) -> Result<(Export, NewOverrides), Error> {
        let next_starting = match i < (self.asset_data.exports.len() - 1) {
            true => match &self.asset_data.exports[i + 1] {
                Export::BaseExport(next_export) => next_export.serial_offset as u64,
                _ => reader.data_length()? - 4,
            },
            false => reader.data_length()? - 4,
        };

        reader.seek(SeekFrom::Start(base_export.serial_offset as u64))?;

        let mut new_overrides = NewOverrides::default();

        //todo: manual skips
        let export_class_type = reader
            .get_export_class_type(base_export.class_index)
            .ok_or_else(|| Error::invalid_package_index("Unknown class type".to_string()))?;

        let content = export_class_type.get_content();
        println!("Export class type: {}", content);
        let mut export: Export = match export_class_type.get_content().as_str() {
            "Level" => LevelExport::from_base(base_export, reader, next_starting)?.into(),
            "StringTable" => StringTableExport::from_base(base_export, reader)?.into(),
            "Enum" | "UserDefinedEnum" => EnumExport::from_base(base_export, reader)?.into(),
            "Function" => FunctionExport::from_base(base_export, reader)?.into(),
            _ => {
                if export_class_type.get_content().ends_with("DataTable") {
                    DataTableExport::from_base(base_export, reader)?.into()
                } else if export_class_type.get_content().ends_with("StringTable") {
                    StringTableExport::from_base(base_export, reader)?.into()
                } else if export_class_type
                    .get_content()
                    .ends_with("BlueprintGeneratedClass")
                {
                    let class_export = ClassExport::from_base(base_export, reader)?;

                    for entry in &class_export.struct_export.loaded_properties {
                        if let FProperty::FMapProperty(map) = entry {
                            let key_override = match &*map.key_prop {
                                FProperty::FStructProperty(struct_property) => {
                                    match struct_property.struct_value.is_import() {
                                        true => reader
                                            .get_import(struct_property.struct_value)
                                            .map(|e| e.object_name.get_content()),
                                        false => None,
                                    }
                                }
                                _ => None,
                            };
                            if let Some(key) = key_override {
                                new_overrides
                                    .map_key_overrides
                                    .insert(map.generic_property.name.get_content(), key);
                            }

                            let value_override = match &*map.value_prop {
                                FProperty::FStructProperty(struct_property) => {
                                    match struct_property.struct_value.is_import() {
                                        true => reader
                                            .get_import(struct_property.struct_value)
                                            .map(|e| e.object_name.get_content()),
                                        false => None,
                                    }
                                }
                                _ => None,
                            };

                            if let Some(value) = value_override {
                                new_overrides
                                    .map_value_overrides
                                    .insert(map.generic_property.name.get_content(), value);
                            }
                        }
                    }
                    class_export.into()
                } else if export_class_type.get_content().ends_with("Property") {
                    PropertyExport::from_base(base_export, reader)?.into()
                } else {
                    NormalExport::from_base(base_export, reader)?.into()
                }
            }
        };

        let extras_len = next_starting as i64 - reader.position() as i64;
        if extras_len < 0 {
            // todo: warning?

            reader.seek(SeekFrom::Start(base_export.serial_offset as u64))?;
            return Ok((
                RawExport::from_base(base_export.clone(), reader)?.into(),
                new_overrides,
            ));
        } else if let Some(normal_export) = export.get_normal_export_mut() {
            let mut extras = vec![0u8; extras_len as usize];
            reader.read_exact(&mut extras)?;
            normal_export.extras = extras;
        }

        Ok((export, new_overrides))
    }

    /// Write asset header
    fn write_header<Writer: ArchiveWriter>(
        &self,
        cursor: &mut Writer,
        asset_header: &AssetHeader,
    ) -> Result<(), Error> {
        cursor.write_u32::<BigEndian>(UE4_ASSET_MAGIC)?;
        cursor.write_i32::<LittleEndian>(self.legacy_file_version)?;

        if self.legacy_file_version != 4 {
            match self.asset_data.unversioned {
                true => cursor.write_i32::<LittleEndian>(0)?,
                false => cursor.write_i32::<LittleEndian>(864)?,
            };
        }

        match self.asset_data.unversioned {
            true => cursor.write_i32::<LittleEndian>(0)?,
            false => cursor.write_i32::<LittleEndian>(self.asset_data.object_version as i32)?,
        };

        cursor.write_i32::<LittleEndian>(self.asset_data.file_license_version)?;
        if self.legacy_file_version <= -2 {
            match self.asset_data.unversioned {
                true => cursor.write_i32::<LittleEndian>(0)?,
                false => {
                    cursor
                        .write_i32::<LittleEndian>(self.asset_data.custom_versions.len() as i32)?;
                    for custom_version in &self.asset_data.custom_versions {
                        cursor.write_all(&custom_version.guid)?;
                        cursor.write_i32::<LittleEndian>(custom_version.version)?;
                    }
                }
            };
        }

        cursor.write_i32::<LittleEndian>(asset_header.header_offset)?;
        cursor.write_fstring(Some(&self.folder_name))?;
        cursor.write_u32::<LittleEndian>(self.package_flags.bits())?;
        cursor.write_i32::<LittleEndian>(
            self.name_map.get_ref().get_name_map_index_list().len() as i32,
        )?;
        cursor.write_i32::<LittleEndian>(asset_header.name_offset)?;

        if self.asset_data.object_version >= ObjectVersion::VER_UE4_SERIALIZE_TEXT_IN_PACKAGES {
            cursor.write_i32::<LittleEndian>(self.gatherable_text_data_count)?;
            cursor.write_i32::<LittleEndian>(self.gatherable_text_data_offset)?;
        }

        cursor.write_i32::<LittleEndian>(self.asset_data.exports.len() as i32)?;
        cursor.write_i32::<LittleEndian>(asset_header.export_offset)?;
        cursor.write_i32::<LittleEndian>(self.imports.len() as i32)?;
        cursor.write_i32::<LittleEndian>(asset_header.import_offset)?;
        cursor.write_i32::<LittleEndian>(asset_header.depends_offset)?;

        if self.asset_data.object_version >= ObjectVersion::VER_UE4_ADD_STRING_ASSET_REFERENCES_MAP
        {
            cursor.write_i32::<LittleEndian>(self.soft_package_reference_count)?;
            cursor.write_i32::<LittleEndian>(asset_header.soft_package_reference_offset)?;
        }

        if self.asset_data.object_version >= ObjectVersion::VER_UE4_ADDED_SEARCHABLE_NAMES {
            cursor.write_i32::<LittleEndian>(self.searchable_names_offset)?;
        }

        cursor.write_i32::<LittleEndian>(self.thumbnail_table_offset)?;
        cursor.write_all(&self.package_guid)?;
        cursor.write_i32::<LittleEndian>(self.generations.len() as i32)?;

        for _ in 0..self.generations.len() {
            cursor.write_i32::<LittleEndian>(self.asset_data.exports.len() as i32)?;
            cursor.write_i32::<LittleEndian>(
                self.name_map.get_ref().get_name_map_index_list().len() as i32,
            )?;
        }

        if self.asset_data.object_version >= ObjectVersion::VER_UE4_ENGINE_VERSION_OBJECT {
            self.engine_version_recorded.write(cursor)?;
        } else {
            cursor.write_u32::<LittleEndian>(self.engine_version_recorded.build)?;
        }

        if self.asset_data.object_version
            >= ObjectVersion::VER_UE4_PACKAGE_SUMMARY_HAS_COMPATIBLE_ENGINE_VERSION
        {
            self.engine_version_recorded.write(cursor)?;
        }

        cursor.write_u32::<LittleEndian>(self.compression_flags)?;
        cursor.write_i32::<LittleEndian>(0)?; // numCompressedChunks
        cursor.write_u32::<LittleEndian>(self.package_source)?;
        cursor.write_i32::<LittleEndian>(0)?; // numAdditionalPackagesToCook

        if self.legacy_file_version > -7 {
            cursor.write_i32::<LittleEndian>(0)?; // numTextureallocations
        }

        cursor.write_i32::<LittleEndian>(asset_header.asset_registry_data_offset)?;
        cursor.write_i64::<LittleEndian>(asset_header.bulk_data_start_offset)?;

        if self.asset_data.object_version >= ObjectVersion::VER_UE4_WORLD_LEVEL_INFO {
            cursor.write_i32::<LittleEndian>(asset_header.world_tile_info_offset)?;
        }

        if self.asset_data.object_version
            >= ObjectVersion::VER_UE4_CHANGED_CHUNKID_TO_BE_AN_ARRAY_OF_CHUNKIDS
        {
            cursor.write_i32::<LittleEndian>(self.chunk_ids.len() as i32)?;
            for chunk_id in &self.chunk_ids {
                cursor.write_i32::<LittleEndian>(*chunk_id)?;
            }
        } else if self.asset_data.object_version
            >= ObjectVersion::VER_UE4_ADDED_CHUNKID_TO_ASSETDATA_AND_UPACKAGE
        {
            cursor.write_i32::<LittleEndian>(self.chunk_ids[0])?;
        }

        if self.asset_data.object_version
            >= ObjectVersion::VER_UE4_PRELOAD_DEPENDENCIES_IN_COOKED_EXPORTS
        {
            cursor.write_i32::<LittleEndian>(asset_header.preload_dependency_count)?;
            cursor.write_i32::<LittleEndian>(asset_header.preload_dependency_offset)?;
        }

        Ok(())
    }

    /// Write `Export` header
    fn write_export_header<Writer: ArchiveWriter>(
        &self,
        unk: &BaseExport,
        cursor: &mut Writer,
        serial_size: i64,
        serial_offset: i64,
        first_export_dependency_offset: i32,
    ) -> Result<(), Error> {
        cursor.write_i32::<LittleEndian>(unk.class_index.index)?;
        cursor.write_i32::<LittleEndian>(unk.super_index.index)?;

        if self.asset_data.object_version >= ObjectVersion::VER_UE4_TemplateIndex_IN_COOKED_EXPORTS
        {
            cursor.write_i32::<LittleEndian>(unk.template_index.index)?;
        }

        cursor.write_i32::<LittleEndian>(unk.outer_index.index)?;
        cursor.write_fname(&unk.object_name)?;
        cursor.write_u32::<LittleEndian>(unk.object_flags)?;

        if self.asset_data.object_version < ObjectVersion::VER_UE4_64BIT_EXPORTMAP_SERIALSIZES {
            cursor.write_i32::<LittleEndian>(serial_size as i32)?;
            cursor.write_i32::<LittleEndian>(serial_offset as i32)?;
        } else {
            cursor.write_i64::<LittleEndian>(serial_size)?;
            cursor.write_i64::<LittleEndian>(serial_offset)?;
        }

        cursor.write_i32::<LittleEndian>(match unk.forced_export {
            true => 1,
            false => 0,
        })?;
        cursor.write_i32::<LittleEndian>(match unk.not_for_client {
            true => 1,
            false => 0,
        })?;
        cursor.write_i32::<LittleEndian>(match unk.not_for_server {
            true => 1,
            false => 0,
        })?;
        cursor.write_all(&unk.package_guid)?;
        cursor.write_u32::<LittleEndian>(unk.package_flags)?;

        if self.asset_data.object_version >= ObjectVersion::VER_UE4_LOAD_FOR_EDITOR_GAME {
            cursor.write_i32::<LittleEndian>(match unk.not_always_loaded_for_editor_game {
                true => 1,
                false => 0,
            })?;
        }

        if self.asset_data.object_version >= ObjectVersion::VER_UE4_COOKED_ASSETS_IN_EDITOR_SUPPORT
        {
            cursor.write_i32::<LittleEndian>(match unk.is_asset {
                true => 1,
                false => 0,
            })?;
        }

        if self.asset_data.object_version
            >= ObjectVersion::VER_UE4_PRELOAD_DEPENDENCIES_IN_COOKED_EXPORTS
        {
            cursor.write_i32::<LittleEndian>(first_export_dependency_offset)?;
            cursor.write_i32::<LittleEndian>(
                unk.serialization_before_serialization_dependencies.len() as i32,
            )?;
            cursor.write_i32::<LittleEndian>(
                unk.create_before_serialization_dependencies.len() as i32
            )?;
            cursor.write_i32::<LittleEndian>(
                unk.serialization_before_create_dependencies.len() as i32
            )?;
            cursor.write_i32::<LittleEndian>(unk.create_before_create_dependencies.len() as i32)?;
        }
        Ok(())
    }

    /// Rebuild the FName map
    /// This can be used if it's too complicated to keep track of all FNames that were added into the asset
    /// This is useful when copying export from one asset into another
    /// This will automatically figure out every new FName and add them to the name map
    pub fn rebuild_name_map(&mut self) {
        let mut current_name_map = self.name_map.clone();
        self.traverse_fnames(&mut |mut name| {
            let content = name.get_content();
            let FName::Backed { index, number: _, name_map } = &mut name else {
                return;
            };

            if *name_map != current_name_map {
                let new_index = current_name_map
                    .get_mut()
                    .add_name_reference(content, false);

                *index = new_index;
                *name_map = current_name_map.clone();
            }
        });
    }

    /// Write asset data
    pub fn write_data<W: Read + Seek + Write>(
        &self,
        cursor: &mut W,
        uexp_cursor: Option<&mut W>,
    ) -> Result<(), Error> {
        if self.asset_data.use_event_driven_loader != uexp_cursor.is_some() {
            return Err(Error::no_data(format!(
                "use_separate_bulk_data_files is {} but uexp_cursor is {}",
                self.asset_data.use_event_driven_loader,
                match uexp_cursor.is_some() {
                    true => "Some(...)",
                    false => "None",
                }
            )));
        }

        let header = AssetHeader {
            name_offset: self.name_offset,
            import_offset: self.import_offset,
            export_offset: self.export_offset,
            depends_offset: self.depends_offset,
            soft_package_reference_offset: self.soft_package_reference_offset,
            asset_registry_data_offset: self.asset_registry_data_offset,
            world_tile_info_offset: self.world_tile_info_offset,
            preload_dependency_count: 0,
            preload_dependency_offset: self.preload_dependency_offset,
            header_offset: self.header_offset,
            bulk_data_start_offset: self.bulk_data_start_offset,
        };

        let mut raw_serializer = RawWriter::new(
            cursor,
            self.asset_data.object_version,
            self.asset_data.object_version_ue5,
            self.asset_data.use_event_driven_loader,
            self.name_map.clone(),
        );
        let mut serializer = AssetArchiveWriter::new(
            &mut raw_serializer,
            &self.asset_data,
            &self.imports,
            self.name_map.clone(),
        );

        self.write_header(&mut serializer, &header)?;

        let name_offset = match !self.name_map.get_ref().is_empty() {
            true => serializer.position() as i32,
            false => 0,
        };

        for name in self.name_map.get_ref().get_name_map_index_list() {
            serializer.write_fstring(Some(name))?;

            if self.asset_data.object_version >= ObjectVersion::VER_UE4_NAME_HASHES_SERIALIZED {
                match self.override_name_map_hashes.get_by_key(name) {
                    Some(e) => serializer.write_u32::<LittleEndian>(*e)?,
                    None => serializer.write_u32::<LittleEndian>(crc::generate_hash(name))?,
                };
            }
        }

        let import_offset = match !self.imports.is_empty() {
            true => serializer.position() as i32,
            false => 0,
        };

        for import in &self.imports {
            serializer.write_fname(&import.class_package)?;
            serializer.write_fname(&import.class_name)?;
            serializer.write_i32::<LittleEndian>(import.outer_index.index)?;
            serializer.write_fname(&import.object_name)?;
        }

        let export_offset = match !self.asset_data.exports.is_empty() {
            true => serializer.position() as i32,
            false => 0,
        };

        for export in &self.asset_data.exports {
            let unk: &BaseExport = export.get_base_export();
            self.write_export_header(
                unk,
                &mut serializer,
                unk.serial_size,
                unk.serial_offset,
                unk.first_export_dependency_offset,
            )?;
        }

        let depends_offset = match self.depends_map {
            Some(_) => serializer.position() as i32,
            None => 0,
        };

        if let Some(ref map) = self.depends_map {
            for i in 0..self.asset_data.exports.len() {
                let dummy = Vec::new();
                let current_data = match map.get(i) {
                    Some(e) => e,
                    None => &dummy,
                };
                serializer.write_i32::<LittleEndian>(current_data.len() as i32)?;
                for i in current_data {
                    serializer.write_i32::<LittleEndian>(*i)?;
                }
            }
        }

        let soft_package_reference_offset = match self.soft_package_reference_list {
            Some(_) => serializer.position() as i32,
            None => 0,
        };

        if let Some(ref package_references) = self.soft_package_reference_list {
            for reference in package_references {
                serializer.write_fstring(Some(reference))?;
            }
        }

        // todo: asset registry data support
        // we can support it now I think?
        let asset_registry_data_offset = match self.asset_registry_data_offset != 0 {
            true => serializer.position() as i32,
            false => 0,
        };
        if self.asset_registry_data_offset != 0 {
            serializer.write_i32::<LittleEndian>(0)?; // asset registry data length
        }

        let world_tile_info_offset = match self.asset_data.world_tile_info {
            Some(_) => serializer.position() as i32,
            None => 0,
        };

        if let Some(ref world_tile_info) = self.asset_data.world_tile_info {
            world_tile_info.write(&mut serializer)?;
        }

        let mut preload_dependency_count = 0;
        let preload_dependency_offset = serializer.position() as i32;

        if self.asset_data.use_event_driven_loader {
            for export in &self.asset_data.exports {
                let unk_export = export.get_base_export();

                for element in &unk_export.serialization_before_serialization_dependencies {
                    serializer.write_i32::<LittleEndian>(element.index)?;
                }

                for element in &unk_export.create_before_serialization_dependencies {
                    serializer.write_i32::<LittleEndian>(element.index)?;
                }

                for element in &unk_export.serialization_before_create_dependencies {
                    serializer.write_i32::<LittleEndian>(element.index)?;
                }

                for element in &unk_export.create_before_create_dependencies {
                    serializer.write_i32::<LittleEndian>(element.index)?;
                }

                preload_dependency_count += unk_export
                    .serialization_before_serialization_dependencies
                    .len() as i32
                    + unk_export.create_before_serialization_dependencies.len() as i32
                    + unk_export.serialization_before_create_dependencies.len() as i32
                    + unk_export.create_before_create_dependencies.len() as i32;
            }
        } else {
            preload_dependency_count = -1;
        }

        let header_offset = match !self.asset_data.exports.is_empty() {
            true => serializer.position() as i32,
            false => 0,
        };

        let mut category_starts = Vec::with_capacity(self.asset_data.exports.len());

        let final_cursor_pos = serializer.position();

        let mut raw_bulk_serializer = match self.asset_data.use_event_driven_loader {
            true => Some(RawWriter::new(
                uexp_cursor.unwrap(),
                self.asset_data.object_version,
                self.asset_data.object_version_ue5,
                self.asset_data.use_event_driven_loader,
                self.name_map.clone(),
            )),
            false => None,
        };

        let mut bulk_serializer = match self.asset_data.use_event_driven_loader {
            true => Some(AssetArchiveWriter::new(
                raw_bulk_serializer.as_mut().unwrap(),
                &self.asset_data,
                &self.imports,
                self.name_map.clone(),
            )),
            false => None,
        };

        let bulk_serializer = match self.asset_data.use_event_driven_loader {
            true => bulk_serializer.as_mut().unwrap(),
            false => &mut serializer,
        };

        for export in &self.asset_data.exports {
            category_starts.push(match self.asset_data.use_event_driven_loader {
                true => bulk_serializer.position() + final_cursor_pos,
                false => bulk_serializer.position(),
            });
            export.write(bulk_serializer)?;
            if let Some(normal_export) = export.get_normal_export() {
                bulk_serializer.write_all(&normal_export.extras)?;
            }
        }
        bulk_serializer.write_all(&[0xc1, 0x83, 0x2a, 0x9e])?;

        let bulk_data_start_offset = match self.asset_data.use_event_driven_loader {
            true => final_cursor_pos as i64 + bulk_serializer.position() as i64,
            false => serializer.position() as i64,
        } - 4;

        if !self.asset_data.exports.is_empty() {
            serializer.seek(SeekFrom::Start(export_offset as u64))?;
            let mut first_export_dependency_offset = 0;
            for i in 0..self.asset_data.exports.len() {
                let unk = &self.asset_data.exports[i].get_base_export();
                let next_loc = match self.asset_data.exports.len() - 1 > i {
                    true => category_starts[i + 1] as i64,
                    false => bulk_data_start_offset,
                };
                self.write_export_header(
                    unk,
                    &mut serializer,
                    next_loc - category_starts[i] as i64,
                    category_starts[i] as i64,
                    match self.asset_data.use_event_driven_loader {
                        true => first_export_dependency_offset,
                        false => -1,
                    },
                )?;
                first_export_dependency_offset +=
                    (unk.serialization_before_serialization_dependencies.len()
                        + unk.create_before_serialization_dependencies.len()
                        + unk.serialization_before_create_dependencies.len()
                        + unk.create_before_create_dependencies.len()) as i32;
            }
        }

        serializer.seek(SeekFrom::Start(0))?;

        let header = AssetHeader {
            name_offset,
            import_offset,
            export_offset,
            depends_offset,
            soft_package_reference_offset,
            asset_registry_data_offset,
            world_tile_info_offset,
            preload_dependency_count,
            preload_dependency_offset,
            header_offset,
            bulk_data_start_offset,
        };
        self.write_header(&mut serializer, &header)?;

        serializer.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

// custom debug implementation to not print the whole data buffer
impl Debug for Asset {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("Asset")
            .field("info", &self.info)
            .field("asset_data", &self.asset_data)
            .field("legacy_file_version", &self.legacy_file_version)
            // imports
            // exports
            // depends map
            // soft package reference list
            // asset registry data
            // world tile info
            // preload dependencies
            .field("generations", &self.generations)
            .field("package_guid", &self.package_guid)
            .field("engine_version", &self.asset_data.get_engine_version())
            .field("engine_version_recorded", &self.engine_version_recorded)
            .field("engine_version_compatible", &self.engine_version_compatible)
            .field("chunk_ids", &self.chunk_ids)
            .field("package_flags", &self.package_flags)
            .field("package_source", &self.package_source)
            .field("folder_name", &self.folder_name)
            // map struct type override
            // override name map hashes
            .field("header_offset", &self.header_offset)
            .field("name_count", &self.name_count)
            .field("name_offset", &self.name_offset)
            .field(
                "gatherable_text_data_count",
                &self.gatherable_text_data_count,
            )
            .field(
                "gatherable_text_data_offset",
                &self.gatherable_text_data_offset,
            )
            .field("export_count", &self.export_count)
            .field("export_offset", &self.export_offset)
            .field("import_count", &self.import_count)
            .field("import_offset", &self.import_offset)
            .field("depends_offset", &self.depends_offset)
            .field(
                "soft_package_reference_count",
                &self.soft_package_reference_count,
            )
            .field(
                "soft_package_reference_offset",
                &self.soft_package_reference_offset,
            )
            .field("searchable_names_offset", &self.searchable_names_offset)
            .field("thumbnail_table_offset", &self.thumbnail_table_offset)
            .field("compression_flags", &self.compression_flags)
            .field(
                "asset_registry_data_offset",
                &self.asset_registry_data_offset,
            )
            .field("bulk_data_start_offset", &self.bulk_data_start_offset)
            .field("world_tile_info_data_offset", &self.world_tile_info_offset)
            .field("preload_dependency_count", &self.preload_dependency_count)
            .field("preload_dependency_offset", &self.preload_dependency_offset)
            .finish()
    }
}

/// EngineVersion for an Asset
#[derive(Debug, Clone)]
pub struct FEngineVersion {
    major: u16,
    minor: u16,
    patch: u16,
    build: u32,
    branch: Option<String>,
}
impl FEngineVersion {
    fn new(major: u16, minor: u16, patch: u16, build: u32, branch: Option<String>) -> Self {
        Self {
            major,
            minor,
            patch,
            build,
            branch,
        }
    }

    fn read<Reader: ArchiveReader>(cursor: &mut Reader) -> Result<Self, Error> {
        let major = cursor.read_u16::<LittleEndian>()?;
        let minor = cursor.read_u16::<LittleEndian>()?;
        let patch = cursor.read_u16::<LittleEndian>()?;
        let build = cursor.read_u32::<LittleEndian>()?;
        let branch = cursor.read_fstring()?;

        Ok(Self::new(major, minor, patch, build, branch))
    }

    fn write<Writer: ArchiveWriter>(&self, cursor: &mut Writer) -> Result<(), Error> {
        cursor.write_u16::<LittleEndian>(self.major)?;
        cursor.write_u16::<LittleEndian>(self.minor)?;
        cursor.write_u16::<LittleEndian>(self.patch)?;
        cursor.write_u32::<LittleEndian>(self.build)?;
        cursor.write_fstring(self.branch.as_deref())?;
        Ok(())
    }

    fn unknown() -> Self {
        Self::new(0, 0, 0, 0, None)
    }
}
