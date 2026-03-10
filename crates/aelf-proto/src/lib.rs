#![forbid(unsafe_code)]

pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/_includes.rs"));
}

macro_rules! export_proto_mod {
    ($module:ident, $file:literal) => {
        pub mod $module {
            pub use super::generated::$module::*;

            include!(concat!(env!("OUT_DIR"), "/", $file, ".serde.rs"));
        }
    };
}

macro_rules! export_proto_alias_mod {
    ($module:ident, $generated:ident, $file:literal) => {
        pub mod $module {
            pub use super::generated::$generated::*;

            include!(concat!(env!("OUT_DIR"), "/", $file, ".serde.rs"));
        }
    };
}

export_proto_mod!(acs0, "acs0");
export_proto_mod!(acs1, "acs1");
export_proto_mod!(acs10, "acs10");
export_proto_mod!(acs2, "acs2");
export_proto_mod!(acs3, "acs3");
export_proto_mod!(acs4, "acs4");
export_proto_mod!(acs5, "acs5");
export_proto_mod!(acs7, "acs7");
export_proto_mod!(acs8, "acs8");
export_proto_mod!(acs9, "acs9");
export_proto_mod!(aelf, "aelf");
export_proto_mod!(association, "association");
export_proto_mod!(configuration, "configuration");
export_proto_mod!(cross_chain, "cross_chain");
export_proto_mod!(economic, "economic");
export_proto_mod!(election, "election");
export_proto_mod!(parliament, "parliament");
export_proto_mod!(profit, "profit");
export_proto_mod!(referendum, "referendum");
export_proto_mod!(token, "token");
export_proto_mod!(token_converter, "token_converter");
export_proto_mod!(token_holder, "token_holder");
export_proto_mod!(treasury, "treasury");
export_proto_mod!(vote, "vote");
export_proto_mod!(zero, "zero");
export_proto_alias_mod!(aedpos, aed_po_s, "aed_po_s");

pub const FILE_DESCRIPTOR_SET: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/aelf_descriptor.bin"));
