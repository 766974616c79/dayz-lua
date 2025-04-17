#![cfg(windows)]
#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use lazy_static::lazy_static;
use mlua::{
    ffi::{luaL_checkstring, lua_pushinteger, lua_type, LUA_TSTRING},
    lua_State, Function, RegistryKey,
};
use once_cell::sync::OnceCell;
use std::{
    collections::HashMap,
    os::raw::{c_char, c_void},
    sync::{LazyLock, Mutex},
    time::Duration,
};
use windows::{
    core::{BOOL, PCSTR},
    Win32::{
        Foundation::HANDLE,
        System::{
            Console::AllocConsole,
            LibraryLoader::GetModuleHandleA,
            SystemServices::{
                DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH, DLL_THREAD_ATTACH, DLL_THREAD_DETACH,
            },
        },
    },
};

static LUA: OnceCell<Mutex<mlua::Lua>> = OnceCell::new();

lazy_static! {
    static ref Hooks: Mutex<HashMap<String, OnceCell<RegistryKey>>> = Mutex::new(HashMap::new());
}

type fn_Print = unsafe fn(a1: i64, a2: *const c_char) -> i64;

unsafe extern "C-unwind" fn print(state: *mut lua_State) -> i32 {
    let handle = GetModuleHandleA(PCSTR::null()).unwrap();
    let ori: fn_Print = std::mem::transmute::<usize, fn_Print>(handle.0 as usize + 0x754CA0);
    match lua_type(state, 1) {
        LUA_TSTRING => {
            ori(1, luaL_checkstring(state, 1));
        }
        _ => {}
    }

    1
}

macro_rules! generate_function {
    ($sig:literal, $name:ident, $sig_name:ident, $return:ty) => {
        static $sig_name: LazyLock<usize> = LazyLock::new(|| unsafe {
            skidscan::signature!($sig)
                .scan_module("DayZServer_x64.exe")
                .unwrap() as usize
        });

        unsafe extern "C-unwind" fn $name(state: *mut lua_State) -> i32 {
            lua_pushinteger(
                state,
                std::mem::transmute::<usize, fn() -> $return>(*$sig_name)(),
            );

            1
        }
    };
}

generate_function!("48 8B 05 ? ? ? ? C3 CC CC CC CC CC CC CC CC 48 8B 05 ? ? ? ? C3 CC CC CC CC CC CC CC CC 48 8B 05 ? ? ? ? C3 CC CC CC CC CC CC CC CC 8B 41", GetGame, GetGameSig, i64);
generate_function!(
    "48 8B 05 ? ? ? ? C3 CC CC CC CC CC CC CC CC 48 8B C4",
    GetWorld,
    GetWorldSig,
    i64
);

#[no_mangle]
unsafe extern "system" fn DllMain(_hinst: HANDLE, reason: u32, _reserved: *mut c_void) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            std::thread::spawn(move || {
                AllocConsole().unwrap();

                let lua = LUA
                    .get_or_init(|| Mutex::new(mlua::Lua::new()))
                    .lock()
                    .unwrap();

                let get_game = lua.create_c_function(GetGame).unwrap();
                lua.globals().raw_set("GetGame", get_game).unwrap();

                let get_world = lua.create_c_function(GetWorld).unwrap();
                lua.globals().raw_set("GetWorld", get_world).unwrap();

                let print = lua.create_c_function(print).unwrap();
                lua.globals().raw_set("print", print).unwrap();

                let hook_add = lua
                    .create_function(move |lua, (name, callback): (String, Function)| {
                        let c = OnceCell::new();
                        c.set(lua.create_registry_value(callback).unwrap()).unwrap();

                        Hooks.lock().unwrap().insert(name, c);

                        Ok(())
                    })
                    .unwrap();

                let hook_run = lua
                    .create_function(move |lua, name: String| {
                        let hooks = Hooks.lock().unwrap();
                        let r = hooks.get(&name).unwrap().get().unwrap();
                        lua.registry_value::<Function>(r)
                            .unwrap()
                            .call::<()>(())
                            .unwrap();

                        Ok(())
                    })
                    .unwrap();

                let hooks = lua
                    .create_table_from(vec![("Add", hook_add), ("Run", hook_run)])
                    .unwrap();

                std::thread::sleep(Duration::from_secs(3));

                lua.globals().raw_set("hooks", hooks).unwrap();
                lua.load(
                    r#"
                print(tostring(GetGame()))
                print(tostring(GetWorld()))"#,
                )
                .exec()
                .unwrap();
            });
        }
        DLL_PROCESS_DETACH => {
            println!("detaching");
        }
        DLL_THREAD_ATTACH => {}
        DLL_THREAD_DETACH => {}
        _ => {}
    };

    BOOL::from(true)
}
