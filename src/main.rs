#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_mut)]

mod module;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use module::ModuleProvider;

// This function creates a promise, escapes the resolver
// and stores the resolver in a list. The list is stored
// in a v8 external and the memory is owned by rust::main.
fn resolve_later_fn(
    mut scope: &mut v8::HandleScope<'_>,
    mut args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let scope = &mut v8::EscapableHandleScope::new(scope);
    let resolver = v8::PromiseResolver::new(scope).unwrap();
    let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
    retval.set(promise.into());

    unsafe {
        let raw_data = v8::Local::<v8::External>::cast(args.data());
        let pp = &mut *(raw_data.value()
            as *mut Mutex<HashMap<String, v8::Local<'_, v8::PromiseResolver>>>);
        let id = uuid::Uuid::new_v4().to_string();
        let mut l = pp.lock().unwrap();
        l.insert(id, scope.escape(resolver));
    }
}

extern "C" fn promise_hook_update(
    t: v8::PromiseHookType,
    p: v8::Local<'_, v8::Promise>,
    v: v8::Local<'_, v8::Value>,
) {
    let scope = &mut unsafe { v8::CallbackScope::new(p) };
    let context = p.get_creation_context(scope).unwrap();
    let scope = &mut v8::ContextScope::new(scope, context);
    println!(
        "Update {:?} {:?} {:?} {}",
        t,
        p,
        v.to_string(scope).unwrap().to_rust_string_lossy(scope),
        v.is_promise()
    );
}

fn main() {
    v8::V8::set_flags_from_string("--harmony --single-threaded");
    let platform = v8::Platform::new_single_threaded(false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();

    let mut isolate = v8::Isolate::new(v8::CreateParams::default());

    //isolate.set_promise_hook(promise_hook_update);

    let mut handle_scope = v8::HandleScope::new(&mut isolate);

    let mut pending_promises: Mutex<HashMap<String, v8::Local<'_, v8::PromiseResolver>>> =
        Mutex::new(HashMap::new());

    let external = v8::External::new(
        &mut handle_scope,
        &mut pending_promises as *mut _ as *mut std::ffi::c_void,
    );

    let global = v8::ObjectTemplate::new(&mut handle_scope);

    global.set(
        v8::String::new(&mut handle_scope, "resolve_later")
            .unwrap()
            .into(),
        v8::FunctionTemplate::builder(resolve_later_fn)
            .data(external.into())
            .build(&mut handle_scope)
            .into(),
    );

    let context = v8::Context::new_from_template(&mut handle_scope, global);
    let mut context_scope = v8::ContextScope::new(&mut handle_scope, context);

    let module = ModuleProvider::create_module_from_source(
        &mut context_scope,
        String::from(
            r#"
export async function doit() {
    let value = await resolve_later();
    return `Resolution Value = ${value}`;
}
"#,
        ),
    );

    let mut top_level_promises: Vec<v8::Local<'_, v8::Promise>> = Vec::new();

    let ns = v8::Local::<v8::Object>::try_from(module.get_module_namespace()).unwrap();
    let nsvalue: v8::Local<'_, v8::Value> = ns.into();

    let name = v8::String::new(&mut context_scope, "doit").unwrap();
    let main = ns.get(&mut context_scope, name.into()).unwrap();
    let main_fn = v8::Local::<v8::Function>::try_from(main).unwrap();

    let mut try_catch = v8::TryCatch::new(&mut context_scope);

    for _i in 0..10000 {
        let result = main_fn.call(&mut try_catch, nsvalue, &[]);

        match result {
            Some(v) => {
                if v.is_promise() {
                    let promise = v8::Local::<'_, v8::Promise>::try_from(v)
                        .expect("Function did not return promise as expected.");
                    top_level_promises.push(promise);
                } else {
                    panic!("Should be a promise");
                }
            }
            None => {
                panic!("No Response");
            }
        };
    }

    for _iteration in 0..10 {
        let mut pendresrej: [u32; 3] = [0, 0, 0];

        let pp = pending_promises.lock().unwrap();
        for (id, resolver) in pp.iter() {
            let p = resolver.get_promise(&mut try_catch);
            let state = p.state();

            match state {
                v8::PromiseState::Pending => pendresrej[0] += 1,
                v8::PromiseState::Fulfilled => pendresrej[1] += 1,
                v8::PromiseState::Rejected => pendresrej[2] += 1,
            }
        }

        println!("{:?}", pendresrej);
    }

    println!("Done");
}
