## Multiple promise resolution in v8 with Rust

My goal is to create a v8 Isolate that can create multiple promises, and resolve them concurrently (in a single threaded fashion). I'd like to have "multiple promises in flight" at the same time, but I will resolve them one at a time in the isolate (just like a browser, no javascript multithreading).

My initial experiments worked great with a single promise at a time, but this meant promises with resolutions that took long periods blocked the resolution of other promises that might be faster. As I started to experiment I started getting segfaults and other odd issues.

I wrote this repo to experiment and figure out what I was doing wrong.

Things I'm experiencing attempting to build a system that works:
 - Promises resolving randomly, without any action on the part of my code.
 - Promises rejecting randomly, without any action on the part of my code.
 - Random SegFaults
 
I've tried a number of different `v8::TryCatch` and `v8::EscapableHandleScope` solutions, but none have been able to resolve my issue. Hopefully there is someone out there that can help me understand what I'm doing wrong.

| This feels like a memory mishandling error but I'm unable to see what I'm doing wrong.

-- This repo has working code that will recreate the issue (hopefully), keep in mind I'm experiencing this at random, so you may need to run it a couple times in order to see the issue. --

## External Function and Promise Creation

I'm exposing a rust function that creates a `v8::Promise`, stores the promise resolver in a `v8::External` and returns. I realize the mutex might be not needed here.

```rust
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
```

Assuming this function is called by javascript with something like:

```javascript
export async function doit() {
    let value = await resolve_later();
    return `Resolution Value = ${value}`;
}
```

I'm assuming I will now have a `v8::Promise`, in a pending state, and a `HashMap` with the `v8::PromiseResolver` in it. If I do nothing, that promise should never resolve.

To be clear, there are now 2 promises, the top level promise created by `doit` and a inner promise created by `resolve_later()`.

## Initialize Isolate and Compile the module

```rust
fn main() {
    v8::V8::set_flags_from_string("--harmony --single-threaded");
    let platform = v8::Platform::new_single_threaded(false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();

    let mut isolate = v8::Isolate::new(v8::CreateParams::default());
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
```

`resolve_later` is now a function available to the context and the data of the `Mutex<HashMap>` is available to it. In all my experiments this part works well and works as one migth expect, although it does require some `unsafe` to work.

```rust
    let mut context_scope = v8::ContextScope::new(&mut handle_scope, context);

    // This helper code is located in module.rs in the repo.
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

    let ns = v8::Local::<v8::Object>::try_from(module.get_module_namespace()).unwrap();
    let nsvalue: v8::Local<'_, v8::Value> = ns.into();

    let name = v8::String::new(&mut context_scope, "doit").unwrap();
    let main = ns.get(&mut context_scope, name.into()).unwrap();
    let main_fn = v8::Local::<v8::Function>::try_from(main).unwrap();

}
```

We now have a module and a v8::Function we can call to create a `v8::Local<v8::Promise>`.

```rust
    let mut top_level_promises: Vec<v8::Local<'_, v8::Promise>> = Vec::new();

    for _i in 0..10 {
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
```

We've called our function 10 times, and stored the resulting promises in a Vector, the assumption is that none of these promises will remain in a `Pending` state until we resolve them.

Thus

```rust
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
```

Should usually show 10 pending promises, however, __seemingly__ at random, 2 promises will be auto-rejected or auto-resolved with odd data.

No matter how many promises we create, it's always 2 that are auto-resolve/rejected. I would expect the fixed number of 2 to change based on the total number of promises if it was a simple memory leak or mishandling of internal data.

EVEN more peculiar, is that if i run this rust code in debug:

`cargo run`

The number of promises auto-resolving is 1, if I run the code with:

`cargo run --release`

The number of promises auto-resolving is 2.

There isn't ALWAYS an error, but when there is, it's always the same count.

This behavior is common when this loop runs:

pending: 10
fulfilled: 0
rejected: 0

However, other times it is:

pending: 8
fulfilled: 2
rejected: 0

and other times:

pending: 8
fulfilled: 0
rejected: 2

Without any changes or recompiles.

I'm stumped, I'm almost certain I'm simply using some v8 concept wrong, but I've spent days trying to figure out what that is and I can't determine it, any help would be greatly appreciated.

----

I believe the SegFaults I'm experiencing occur when I'm attempting to resolve these promises, but I don't think it's worth investigating those until I understand what is going on here.
