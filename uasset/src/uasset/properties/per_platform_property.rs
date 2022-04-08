use std::io::{Cursor, ErrorKind};
use std::mem::size_of;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use ordered_float::OrderedFloat;

use crate::uasset::error::Error;
use crate::{uasset::{unreal_types::{Guid, FName}, cursor_ext::CursorExt, Asset}, optional_guid, optional_guid_write, impl_property_data_trait};
use crate::uasset::properties::{PropertyTrait, PropertyDataTrait};

#[derive(Hash, PartialEq, Eq)]
pub struct PerPlatformBoolProperty {
    pub name: FName,
    pub property_guid: Option<Guid>,
    pub duplication_index: i32,
    pub value: Vec<bool>,
}
impl_property_data_trait!(PerPlatformBoolProperty);

#[derive(Hash, PartialEq, Eq)]
pub struct PerPlatformIntProperty {
    pub name: FName,
    pub property_guid: Option<Guid>,
    pub duplication_index: i32,
    pub value: Vec<i32>,
}
impl_property_data_trait!(PerPlatformIntProperty);

#[derive(Hash, PartialEq, Eq)]
pub struct PerPlatformFloatProperty {
    pub name: FName,
    pub property_guid: Option<Guid>,
    pub duplication_index: i32,
    pub value: Vec<OrderedFloat<f32>>,
}
impl_property_data_trait!(PerPlatformFloatProperty);

impl PerPlatformBoolProperty {
    pub fn new(asset: &mut Asset, name: FName, include_header: bool, length: i64, duplication_index: i32) -> Result<Self, Error> {
        let property_guid = optional_guid!(asset, include_header);

        let num_entries = asset.cursor.read_i32::<LittleEndian>()?;
        let mut value = Vec::with_capacity(num_entries as usize);

        for i in 0..num_entries as usize {
            value.push(asset.cursor.read_bool()?);
        }

        Ok(PerPlatformBoolProperty {
            name,
            property_guid,
            duplication_index,
            value,
        })
    }
}

impl PropertyTrait for PerPlatformBoolProperty {
    fn write(&self, asset: &Asset, cursor: &mut Cursor<Vec<u8>>, include_header: bool) -> Result<usize, Error> {
        optional_guid_write!(self, asset, cursor, include_header);
        cursor.write_i32::<LittleEndian>(self.value.len() as i32)?;
        for entry in &self.value {
            cursor.write_bool(*entry)?;
        }
        Ok(size_of::<i32>() + size_of::<bool>() * self.value.len())
    }
}

impl PerPlatformIntProperty {
    pub fn new(asset: &mut Asset, name: FName, include_header: bool, length: i64, duplication_index: i32) -> Result<Self, Error> {
        let property_guid = optional_guid!(asset, include_header);

        let num_entries = asset.cursor.read_i32::<LittleEndian>()?;
        let mut value = Vec::with_capacity(num_entries as usize);

        for i in 0..num_entries as usize {
            value.push(asset.cursor.read_i32::<LittleEndian>()?);
        }

        Ok(PerPlatformIntProperty {
            name,
            property_guid,
            duplication_index,
            value,
        })
    }
}

impl PropertyTrait for PerPlatformIntProperty {
    fn write(&self, asset: &Asset, cursor: &mut Cursor<Vec<u8>>, include_header: bool) -> Result<usize, Error> {
        optional_guid_write!(self, asset, cursor, include_header);
        cursor.write_i32::<LittleEndian>(self.value.len() as i32)?;
        for entry in &self.value {
            cursor.write_i32::<LittleEndian>(*entry)?;
        }
        Ok(size_of::<i32>() + size_of::<i32>() * self.value.len())
    }
}

impl PerPlatformFloatProperty {
    pub fn new(asset: &mut Asset, name: FName, include_header: bool, length: i64, duplication_index: i32) -> Result<Self, Error> {
        let property_guid = optional_guid!(asset, include_header);

        let num_entries = asset.cursor.read_i32::<LittleEndian>()?;
        let mut value = Vec::with_capacity(num_entries as usize);

        for i in 0..num_entries as usize {
            value.push(OrderedFloat(asset.cursor.read_f32::<LittleEndian>()?));
        }

        Ok(PerPlatformFloatProperty {
            name,
            property_guid,
            duplication_index,
            value,
        })
    }
}

impl PropertyTrait for PerPlatformFloatProperty {
    fn write(&self, asset: &Asset, cursor: &mut Cursor<Vec<u8>>, include_header: bool) -> Result<usize, Error> {
        optional_guid_write!(self, asset, cursor, include_header);
        cursor.write_i32::<LittleEndian>(self.value.len() as i32)?;
        for entry in &self.value {
            cursor.write_f32::<LittleEndian>(entry.0)?;
        }
        Ok(size_of::<i32>() + size_of::<f32>() * self.value.len())
    }
}