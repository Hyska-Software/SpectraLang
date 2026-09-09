use super::*;

pub(crate) fn lookup_std_host_function(path: &[String]) -> Option<HostFunctionDescriptor> {
    match path {
        [] => None,
        [first, ..] if first != "std" => None,
        [prefix, api, db, sqlite, function] if prefix == "std" && api == "api" && db == "db" && sqlite == "sqlite" => {
            lookup_std_api_host_function("db.sqlite", function)
        }
        [prefix, api, db, postgres, function] if prefix == "std" && api == "api" && db == "db" && postgres == "postgres" => {
            lookup_std_api_host_function("db.postgres", function)
        }
        [prefix, api, db, redis, function] if prefix == "std" && api == "api" && db == "db" && redis == "redis" => {
            lookup_std_api_host_function("db.redis", function)
        }
        [prefix, api, db, pool, function] if prefix == "std" && api == "api" && db == "db" && pool == "pool" => {
            lookup_std_api_host_function("db.pool", function)
        }
        [prefix, api, db, migrate, function] if prefix == "std" && api == "api" && db == "db" && migrate == "migrate" => {
            lookup_std_api_host_function("db.migrate", function)
        }
        [prefix, api, module, function] if prefix == "std" && api == "api" => {
            lookup_std_api_host_function(module, function)
        }
        [prefix, module, function] if prefix == "std" => lookup_std_host_module_function(module, function),
        _ => None,
    }
}
