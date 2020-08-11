// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use ethers_contract::Abigen;

use std::env;
use std::path::PathBuf;

fn generate_abi(names: &[&str]) {
    let mut abi_path =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    abi_path.push("abi");

    let mut out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    out_dir.push("abi");

    std::fs::create_dir_all(&out_dir)
        .expect("unable to create output directory");

    for name in names {
        let mut abi_path = abi_path.join(name);
        abi_path.set_extension("abi");

        let mut out_path = out_dir.join(name);
        out_path.set_extension("rs");

        Abigen::new(name, abi_path.to_str().unwrap())
            .expect("unable to load abi")
            .generate()
            .expect("unable to generate rust")
            .write_to_file(out_path)
            .expect("unable to write bindings");
    }
}

fn main() {
    generate_abi(&["Utxo", "Dropsafe"]);
}
