// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use byteorder::{LittleEndian, WriteBytesExt};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use super::distribution::ExtensionModule;
use super::resource::{
    BuiltExtensionModule, BytecodeModule, PackagedModuleBytecode, PackagedModuleSource,
    SourceModule,
};

/// Represents Python resources to embed in a binary.
#[derive(Debug, Default, Clone)]
pub struct EmbeddedPythonResources {
    pub module_sources: BTreeMap<String, PackagedModuleSource>,
    pub module_bytecode_requests: BTreeMap<String, BytecodeModule>,
    pub module_bytecodes: BTreeMap<String, PackagedModuleBytecode>,
    pub all_modules: BTreeSet<String>,
    pub all_packages: BTreeSet<String>,
    pub resources: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    pub extension_modules: BTreeMap<String, ExtensionModule>,
    pub built_extension_modules: BTreeMap<String, BuiltExtensionModule>,
}

/// Represents a single module's data record.
pub struct ModuleEntry {
    pub name: String,
    pub is_package: bool,
    pub source: Option<Vec<u8>>,
    pub bytecode: Option<Vec<u8>>,
}

/// Represents an ordered collection of module entries.
pub type ModuleEntries = Vec<ModuleEntry>;

impl EmbeddedPythonResources {
    /// Add a source module to the collection of embedded source modules.
    pub fn add_source_module(&mut self, module: &SourceModule) {
        self.module_sources.insert(
            module.name.clone(),
            PackagedModuleSource {
                source: module.source.clone(),
                is_package: module.is_package,
            },
        );

        self.all_modules.insert(module.name.clone());

        if module.is_package {
            self.all_packages.insert(module.name.clone());
        }
    }

    /// Add a bytecode module to the collection of embedded bytecode modules.
    ///
    /// Actual bytecode will be compiled later.
    pub fn add_bytecode_module(&mut self, module: &BytecodeModule) {
        self.module_bytecode_requests
            .insert(module.name.clone(), module.clone());

        self.all_modules.insert(module.name.clone());

        if module.is_package {
            self.all_packages.insert(module.name.clone());
        }
    }

    /// Obtain records for all modules in this resources collection.
    pub fn modules_records(&self) -> ModuleEntries {
        let mut records = ModuleEntries::new();

        for name in &self.all_modules {
            let source = self.module_sources.get(name);
            let bytecode = self.module_bytecodes.get(name);

            records.push(ModuleEntry {
                name: name.clone(),
                is_package: self.all_packages.contains(name),
                source: match source {
                    Some(value) => Some(value.source.clone()),
                    None => None,
                },
                bytecode: match bytecode {
                    Some(value) => Some(value.bytecode.clone()),
                    None => None,
                },
            });
        }

        records
    }

    pub fn write_blobs(
        &self,
        module_names_path: &PathBuf,
        modules_path: &PathBuf,
        resources_path: &PathBuf,
    ) {
        let mut fh = fs::File::create(module_names_path).expect("error creating file");
        for name in &self.all_modules {
            fh.write_all(name.as_bytes()).expect("failed to write");
            fh.write_all(b"\n").expect("failed to write");
        }

        let fh = fs::File::create(modules_path).unwrap();
        write_modules_entries(&fh, &self.modules_records()).unwrap();

        let fh = fs::File::create(resources_path).unwrap();
        write_resources_entries(&fh, &self.resources).unwrap();
    }

    pub fn embedded_extension_module_names(&self) -> BTreeSet<String> {
        let mut res = BTreeSet::new();

        for name in self.extension_modules.keys() {
            res.insert(name.clone());
        }
        for name in self.built_extension_modules.keys() {
            res.insert(name.clone());
        }

        res
    }
}

/// Serialize a ModulesEntries to a writer.
///
/// See the documentation in the `pyembed` crate for the data format.
pub fn write_modules_entries<W: Write>(
    mut dest: W,
    entries: &[ModuleEntry],
) -> std::io::Result<()> {
    dest.write_u32::<LittleEndian>(entries.len() as u32)?;

    for entry in entries.iter() {
        let name_bytes = entry.name.as_bytes();
        dest.write_u32::<LittleEndian>(name_bytes.len() as u32)?;
        dest.write_u32::<LittleEndian>(if let Some(ref v) = entry.source {
            v.len() as u32
        } else {
            0
        })?;
        dest.write_u32::<LittleEndian>(if let Some(ref v) = entry.bytecode {
            v.len() as u32
        } else {
            0
        })?;

        let mut flags = 0;
        if entry.is_package {
            flags |= 1;
        }

        dest.write_u32::<LittleEndian>(flags)?;
    }

    for entry in entries.iter() {
        let name_bytes = entry.name.as_bytes();
        dest.write_all(name_bytes)?;
    }

    for entry in entries.iter() {
        if let Some(ref v) = entry.source {
            dest.write_all(v.as_slice())?;
        }
    }

    for entry in entries.iter() {
        if let Some(ref v) = entry.bytecode {
            dest.write_all(v.as_slice())?;
        }
    }

    Ok(())
}

/// Serializes resource data to a writer.
///
/// See the documentation in the `pyembed` crate for the data format.
pub fn write_resources_entries<W: Write>(
    mut dest: W,
    entries: &BTreeMap<String, BTreeMap<String, Vec<u8>>>,
) -> std::io::Result<()> {
    dest.write_u32::<LittleEndian>(entries.len() as u32)?;

    // All the numeric index data is written in pass 1.
    for (package, resources) in entries {
        let package_bytes = package.as_bytes();

        dest.write_u32::<LittleEndian>(package_bytes.len() as u32)?;
        dest.write_u32::<LittleEndian>(resources.len() as u32)?;

        for (name, value) in resources {
            let name_bytes = name.as_bytes();

            dest.write_u32::<LittleEndian>(name_bytes.len() as u32)?;
            dest.write_u32::<LittleEndian>(value.len() as u32)?;
        }
    }

    // All the name strings are written in pass 2.
    for (package, resources) in entries {
        dest.write_all(package.as_bytes())?;

        for name in resources.keys() {
            dest.write_all(name.as_bytes())?;
        }
    }

    // All the resource data is written in pass 3.
    for resources in entries.values() {
        for value in resources.values() {
            dest.write_all(value.as_slice())?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::resource::BytecodeOptimizationLevel;
    use super::*;

    #[test]
    fn test_add_source_module() {
        let mut v = EmbeddedPythonResources::default();
        v.add_source_module(&SourceModule {
            name: "foo.bar".to_string(),
            source: vec![],
            is_package: false,
        });

        assert_eq!(v.module_sources.len(), 1);
        assert!(v.module_sources.contains_key("foo.bar"));
        assert!(v.all_modules.contains("foo.bar"));
        assert!(!v.all_packages.contains("foo.bar"));

        v.add_source_module(&SourceModule {
            name: "foo".to_string(),
            source: vec![],
            is_package: true,
        });
        assert!(v.module_sources.contains_key("foo"));
        assert!(v.all_modules.contains("foo"));
        assert!(v.all_packages.contains("foo"));
    }

    #[test]
    fn test_add_bytecode_module() {
        let mut v = EmbeddedPythonResources::default();
        v.add_bytecode_module(&BytecodeModule {
            name: "foo.bar".to_string(),
            source: vec![],
            optimize_level: BytecodeOptimizationLevel::Zero,
            is_package: false,
        });

        assert_eq!(v.module_bytecode_requests.len(), 1);
        assert!(v.module_bytecode_requests.contains_key("foo.bar"));
        assert!(v.all_modules.contains("foo.bar"));
        assert!(!v.all_packages.contains("foo.bar"));
    }
}
