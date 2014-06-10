use toml;
use std::collections::HashMap;
use serialize::Decodable;

use core::{Summary,Manifest,Target,Project,Dependency};
use util::{CargoResult,Require,simple_human,toml_error};

pub fn to_manifest(contents: &[u8]) -> CargoResult<Manifest> {
    let root = try!(toml::parse_from_bytes(contents).map_err(|_|
        simple_human("Cargo.toml is not valid Toml")));

    let toml = try!(toml_to_manifest(root).map_err(|_|
        simple_human("Cargo.toml is not a valid Cargo manifest")));

    toml.to_manifest()
}

fn toml_to_manifest(root: toml::Value) -> CargoResult<TomlManifest> {
    fn decode<T: Decodable<toml::Decoder,toml::Error>>(root: &toml::Value, path: &str) -> Result<T, toml::Error> {
        let root = match root.lookup(path) {
            Some(val) => val,
            None => return Err(toml::ParseError)
        };
        toml::from_toml(root.clone())
    }

    let project = try!(decode(&root, "project").map_err(|e| toml_error("ZOMG", e)));
    let lib = decode(&root, "lib").ok();
    let bin = decode(&root, "bin").ok();

    let deps = root.lookup("dependencies");

    let deps = match deps {
        Some(deps) => {
            let table = try!(deps.get_table().require(simple_human("dependencies must be a table"))).clone();

            let mut deps: HashMap<String, TomlDependency> = HashMap::new();

            for (k, v) in table.iter() {
                match v {
                    &toml::String(ref string) => { deps.insert(k.clone(), SimpleDep(string.clone())); },
                    &toml::Table(ref table) => {
                        let mut details = HashMap::<String, String>::new();

                        for (k, v) in table.iter() {
                            let v = try!(v.get_str()
                                         .require(simple_human("dependency values must be string")));

                            details.insert(k.clone(), v.clone());
                        }

                        let version = try!(details.find_equiv(&"version")
                                           .require(simple_human("dependencies must include a version"))).clone();

                        deps.insert(k.clone(), DetailedDep(DetailedTomlDependency {
                            version: version,
                            other: details
                        }));
                    },
                    _ => ()
                }
            }

            Some(deps)
        },
        None => None
    };

    Ok(TomlManifest { project: box project, lib: lib, bin: bin, dependencies: deps })
}

type TomlLibTarget = TomlTarget;
type TomlBinTarget = TomlTarget;

/*
 * TODO: Make all struct fields private
 */

#[deriving(Encodable,PartialEq,Clone,Show)]
pub enum TomlDependency {
    SimpleDep(String),
    DetailedDep(DetailedTomlDependency)
}

#[deriving(Encodable,PartialEq,Clone,Show)]
pub struct DetailedTomlDependency {
    version: String,
    other: HashMap<String, String>
}

#[deriving(Encodable,PartialEq,Clone)]
pub struct TomlManifest {
    project: Box<Project>,
    lib: Option<Vec<TomlLibTarget>>,
    bin: Option<Vec<TomlBinTarget>>,
    dependencies: Option<HashMap<String, TomlDependency>>,
}

impl TomlManifest {
    pub fn to_manifest(&self) -> CargoResult<Manifest> {

        // Get targets
        let targets = normalize(self.lib.as_ref().map(|l| l.as_slice()), self.bin.as_ref().map(|b| b.as_slice()));

        if targets.is_empty() {
            debug!("manifest has no build targets; project={}", self.project);
        }

        let mut deps = Vec::new();

        // Collect the deps
        match self.dependencies {
            Some(ref dependencies) => {
                for (n, v) in dependencies.iter() {
                    let version = match *v {
                        SimpleDep(ref string) => string.clone(),
                        DetailedDep(ref details) => details.version.clone()
                    };

                    deps.push(try!(Dependency::parse(n.as_slice(), version.as_slice())))
                }
            }
            None => ()
        }

        Ok(Manifest::new(
                &Summary::new(&self.project.to_package_id(), deps.as_slice()),
                targets.as_slice(),
                &Path::new("target")))
    }
}

#[deriving(Decodable,Encodable,PartialEq,Clone,Show)]
struct TomlTarget {
    name: String,
    path: Option<String>
}

fn normalize(lib: Option<&[TomlLibTarget]>, bin: Option<&[TomlBinTarget]>) -> Vec<Target> {
    log!(4, "normalizing toml targets; lib={}; bin={}", lib, bin);

    fn lib_targets(dst: &mut Vec<Target>, libs: &[TomlLibTarget]) {
        let l = &libs[0];
        let path = l.path.clone().unwrap_or_else(|| format!("src/{}.rs", l.name));
        dst.push(Target::lib_target(l.name.as_slice(), &Path::new(path)));
    }

    fn bin_targets(dst: &mut Vec<Target>, bins: &[TomlBinTarget], default: |&TomlBinTarget| -> String) {
        for bin in bins.iter() {
            let path = bin.path.clone().unwrap_or_else(|| default(bin));
            dst.push(Target::bin_target(bin.name.as_slice(), &Path::new(path)));
        }
    }

    let mut ret = Vec::new();

    match (lib, bin) {
        (Some(ref libs), Some(ref bins)) => {
            lib_targets(&mut ret, libs.as_slice());
            bin_targets(&mut ret, bins.as_slice(), |bin| format!("src/bin/{}.rs", bin.name));
        },
        (Some(ref libs), None) => {
            lib_targets(&mut ret, libs.as_slice());
        },
        (None, Some(ref bins)) => {
            bin_targets(&mut ret, bins.as_slice(), |bin| format!("src/{}.rs", bin.name));
        },
        (None, None) => ()
    }

    ret
}
