use super::*;

pub(crate) fn lookup_std_host_function(path: &[String]) -> Option<HostFunctionDescriptor> {
    match path {
        [] => None,
        [first, ..] if first != "std" && first != "spectra" => None,
        [prefix, compat, module, function]
            if compat == "compat"
                && (module == "collections" || module == "env" || module == "fs") =>
        {
            let mut descriptor = lookup_std_host_function(&[
                prefix.clone(),
                module.clone(),
                function.clone(),
            ])?;
            let legacy_collection = module == "collections"
                && matches!(
                    function.as_str(),
                    "list_get"
                        | "list_pop"
                        | "list_pop_front"
                        | "list_remove_at"
                        | "map_get"
                        | "map_remove"
                );
            let legacy_env = module == "env" && matches!(function.as_str(), "env_get" | "env_arg");
            let legacy_fs = module == "fs"
                && matches!(
                    function.as_str(),
                    "fs_read" | "fs_write" | "fs_append" | "fs_exists" | "fs_remove"
            );
            if legacy_collection || legacy_env || legacy_fs {
                descriptor.runtime_name = match (module.as_str(), function.as_str()) {
                    ("fs", "fs_read") => spectra_contract::STD_COMPAT_FS_FS_READ_BINDING,
                    ("fs", "fs_write") => spectra_contract::STD_COMPAT_FS_FS_WRITE_BINDING,
                    ("fs", "fs_append") => spectra_contract::STD_COMPAT_FS_FS_APPEND_BINDING,
                    ("fs", "fs_exists") => spectra_contract::STD_COMPAT_FS_FS_EXISTS_BINDING,
                    ("fs", "fs_remove") => spectra_contract::STD_COMPAT_FS_FS_REMOVE_BINDING,
                    _ => Box::leak(
                        format!("spectra.std.compat.{module}.{function}").into_boxed_str(),
                    ),
                };
                descriptor.return_type = if legacy_collection {
                    IRType::Int
                } else if legacy_env || function == "fs_read" {
                    IRType::String
                } else {
                    IRType::Bool
                };
            }
            Some(descriptor)
        }
        [_, api, db, sqlite, function] if api == "api" && db == "db" && sqlite == "sqlite" => {
            lookup_std_api_host_function("db.sqlite", function)
        }
        [_, api, db, postgres, function] if api == "api" && db == "db" && postgres == "postgres" => {
            lookup_std_api_host_function("db.postgres", function)
        }
        [_, api, db, redis, function] if api == "api" && db == "db" && redis == "redis" => {
            lookup_std_api_host_function("db.redis", function)
        }
        [_, api, db, pool, function] if api == "api" && db == "db" && pool == "pool" => {
            lookup_std_api_host_function("db.pool", function)
        }
        [_, api, db, migrate, function] if api == "api" && db == "db" && migrate == "migrate" => {
            lookup_std_api_host_function("db.migrate", function)
        }
        [_, api, module, function] if api == "api" => {
            lookup_std_api_host_function(module, function)
        }
        [_, module, function] => lookup_std_host_module_function(module, function),
        _ => None,
    }
}
