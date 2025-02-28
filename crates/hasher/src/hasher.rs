use moon_config::package::PackageJson;
use moon_config::tsconfig::TsConfigJson;
use moon_project::{Project, Task};
use moon_utils::path;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetHasher {
    // Task `command`
    command: String,

    // Task `args`
    args: Vec<String>,

    // Task `deps`
    deps: Vec<String>,

    // Environment variables
    env_vars: BTreeMap<String, String>,

    // Input files and globs mapped to a unique hash
    input_hashes: BTreeMap<String, String>,

    // Node.js version
    node_version: String,

    // `package.json` `dependencies`
    package_dependencies: BTreeMap<String, String>,

    // `package.json` `devDependencies`
    package_dev_dependencies: BTreeMap<String, String>,

    // `package.json` `peerDependencies`
    package_peer_dependencies: BTreeMap<String, String>,

    // `project.yml` `dependsOn`
    project_deps: Vec<String>,

    // Task `target`
    target: String,

    // `tsconfig.json` `compilerOptions`
    tsconfig_compiler_options: BTreeMap<String, String>,

    // Version of our hasher
    #[allow(dead_code)]
    version: String,
}

impl TargetHasher {
    pub fn new(node_version: String) -> Self {
        TargetHasher {
            node_version,
            version: String::from("1"),
            ..TargetHasher::default()
        }
    }

    pub fn hash_args(&mut self, passthrough_args: &[String]) {
        if !passthrough_args.is_empty() {
            for arg in passthrough_args {
                self.args.push(arg.clone());
            }

            // Sort vectors to be deterministic
            self.args.sort();
        }
    }

    /// Hash a mapping of input file paths to unique file hashes.
    /// File paths *must* be relative from the workspace root.
    pub fn hash_inputs(&mut self, inputs: BTreeMap<String, String>) {
        for (file, hash) in inputs {
            // Standardize on `/` separators so that the hash is
            // the same between windows and nix machines.
            self.input_hashes
                .insert(path::standardize_separators(&file), hash);
        }
    }

    /// Hash `package.json` dependencies as version changes should bust the cache.
    pub fn hash_package_json(&mut self, package: &PackageJson) {
        if let Some(deps) = &package.dependencies {
            self.package_dependencies.extend(deps.clone());
        }

        if let Some(dev_deps) = &package.dev_dependencies {
            self.package_dev_dependencies.extend(dev_deps.clone());
        }

        if let Some(peer_deps) = &package.peer_dependencies {
            self.package_peer_dependencies.extend(peer_deps.clone());
        }
    }

    /// Hash `dependsOn` from the owning project.
    pub fn hash_project(&mut self, project: &Project) {
        self.project_deps = project.get_dependencies(); // Sorted
    }

    /// Hash `args`, `inputs`, `deps`, and `env` vars from a task.
    pub fn hash_task(&mut self, task: &Task) {
        self.command = task.command.clone();
        self.args = task.args.clone();
        self.deps = task.deps.clone();
        self.target = task.target.clone();

        // Sort vectors to be deterministic
        self.args.sort();
        self.deps.sort();
    }

    /// Hash `tsconfig.json` compiler options that may alter compiled/generated output.
    pub fn hash_tsconfig_json(&mut self, tsconfig: &TsConfigJson) {
        if let Some(compiler_options) = &tsconfig.compiler_options {
            if let Some(module) = &compiler_options.module {
                self.tsconfig_compiler_options
                    .insert("module".to_owned(), format!("{:?}", module));
            }

            if let Some(module_resolution) = &compiler_options.module_resolution {
                self.tsconfig_compiler_options.insert(
                    "module_resolution".to_owned(),
                    format!("{:?}", module_resolution),
                );
            }

            if let Some(target) = &compiler_options.target {
                self.tsconfig_compiler_options
                    .insert("target".to_owned(), format!("{:?}", target));
            }
        }
    }

    /// Convert the hasher and its contents to a SHA256 hash.
    pub fn to_hash(&self) -> String {
        let mut sha = Sha256::new();

        let hash_btree = |tree: &BTreeMap<String, String>, hasher: &mut Sha256| {
            for (k, v) in tree {
                hasher.update(k.as_bytes());
                hasher.update(v.as_bytes());
            }
        };

        let hash_vec = |list: &Vec<String>, hasher: &mut Sha256| {
            for v in list {
                hasher.update(v.as_bytes());
            }
        };

        // Order is important! Do not move things around as it will
        // change the hash and break deterministic builds!
        // Adding/removing is ok though.
        sha.update(self.version.as_bytes());
        sha.update(self.node_version.as_bytes());

        // Task
        sha.update(self.command.as_bytes());
        hash_vec(&self.args, &mut sha);
        hash_vec(&self.deps, &mut sha);
        hash_btree(&self.env_vars, &mut sha);
        hash_btree(&self.input_hashes, &mut sha);

        // Deps
        hash_vec(&self.project_deps, &mut sha);
        hash_btree(&self.package_dependencies, &mut sha);
        hash_btree(&self.package_dev_dependencies, &mut sha);
        hash_btree(&self.package_peer_dependencies, &mut sha);

        // Config
        hash_btree(&self.tsconfig_compiler_options, &mut sha);

        format!("{:x}", sha.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_default_hash() {
        let hasher = TargetHasher::new(String::from("0.0.0"));

        assert_eq!(
            hasher.to_hash(),
            String::from("ae2cf745a63ca5f47a7218ae5b4a8267295305591457a33a79c46754c1dcce0b")
        );
    }

    #[test]
    fn returns_same_hash_if_called_again() {
        let hasher = TargetHasher::new(String::from("0.0.0"));

        assert_eq!(hasher.to_hash(), hasher.to_hash());
    }

    #[test]
    fn returns_different_hash_for_diff_contents() {
        let hasher1 = TargetHasher::new(String::from("0.0.0"));
        let hasher2 = TargetHasher::new(String::from("1.0.0"));

        assert_ne!(hasher1.to_hash(), hasher2.to_hash());
    }

    mod btreemap {
        use super::*;

        #[test]
        fn returns_same_hash_for_same_value_inserted() {
            let mut package1 = PackageJson::default();
            package1.add_dependency("react", "17.0.0", true);

            let mut hasher1 = TargetHasher::new(String::from("0.0.0"));
            hasher1.hash_package_json(&package1);

            let mut hasher2 = TargetHasher::new(String::from("0.0.0"));
            hasher2.hash_package_json(&package1);
            hasher2.hash_package_json(&package1);

            assert_eq!(hasher1.to_hash(), hasher2.to_hash());
        }

        #[test]
        fn returns_same_hash_for_diff_order_insertion() {
            let mut package1 = PackageJson::default();
            package1.add_dependency("react", "17.0.0", true);

            let mut package2 = PackageJson::default();
            package2.add_dependency("react-dom", "17.0.0", true);

            let mut hasher1 = TargetHasher::new(String::from("0.0.0"));
            hasher1.hash_package_json(&package2);
            hasher1.hash_package_json(&package1);

            let mut hasher2 = TargetHasher::new(String::from("0.0.0"));
            hasher2.hash_package_json(&package1);
            hasher2.hash_package_json(&package2);

            assert_eq!(hasher1.to_hash(), hasher2.to_hash());
        }

        #[test]
        fn returns_diff_hash_for_overwritten_value() {
            let mut package1 = PackageJson::default();
            package1.add_dependency("react", "17.0.0", true);

            let mut package2 = PackageJson::default();
            package2.add_dependency("react", "18.0.0", true);

            let mut hasher1 = TargetHasher::new(String::from("0.0.0"));
            hasher1.hash_package_json(&package1);

            let hash1 = hasher1.to_hash();

            hasher1.hash_package_json(&package2);

            let hash2 = hasher1.to_hash();

            assert_ne!(hash1, hash2);
        }
    }

    mod package_json {
        use super::*;

        #[test]
        fn supports_all_dep_types() {
            let mut package = PackageJson::default();
            package.add_dependency("moment", "10.0.0", true);

            let mut hasher1 = TargetHasher::new(String::from("0.0.0"));
            hasher1.hash_package_json(&package);
            let hash1 = hasher1.to_hash();

            package.dev_dependencies =
                Some(BTreeMap::from([("eslint".to_owned(), "8.0.0".to_owned())]));

            let mut hasher2 = TargetHasher::new(String::from("0.0.0"));
            hasher2.hash_package_json(&package);
            let hash2 = hasher2.to_hash();

            package.peer_dependencies =
                Some(BTreeMap::from([("react".to_owned(), "18.0.0".to_owned())]));

            let mut hasher3 = TargetHasher::new(String::from("0.0.0"));
            hasher3.hash_package_json(&package);
            let hash3 = hasher3.to_hash();

            assert_ne!(hash1, hash2);
            assert_ne!(hash1, hash3);
            assert_ne!(hash2, hash3);
        }
    }

    mod tsconfig_json {
        use super::*;

        #[test]
        fn supports_all_dep_types() {
            use moon_config::tsconfig::{CompilerOptions, Module, ModuleResolution, Target};

            let mut tsconfig = TsConfigJson {
                compiler_options: Some(CompilerOptions::default()),
                ..TsConfigJson::default()
            };

            tsconfig.compiler_options.as_mut().unwrap().module = Some(Module::Es2022);

            let mut hasher1 = TargetHasher::new(String::from("0.0.0"));
            hasher1.hash_tsconfig_json(&tsconfig);
            let hash1 = hasher1.to_hash();

            tsconfig
                .compiler_options
                .as_mut()
                .unwrap()
                .module_resolution = Some(ModuleResolution::NodeNext);

            let mut hasher2 = TargetHasher::new(String::from("0.0.0"));
            hasher2.hash_tsconfig_json(&tsconfig);
            let hash2 = hasher2.to_hash();

            tsconfig.compiler_options.as_mut().unwrap().target = Some(Target::Es2019);

            let mut hasher3 = TargetHasher::new(String::from("0.0.0"));
            hasher3.hash_tsconfig_json(&tsconfig);
            let hash3 = hasher3.to_hash();

            assert_ne!(hash1, hash2);
            assert_ne!(hash1, hash3);
            assert_ne!(hash2, hash3);
        }
    }
}
