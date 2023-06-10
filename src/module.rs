pub struct ModuleProvider {}

impl ModuleProvider {
    /// javascript allows for dynamic imports via a method titled
    /// import. for now this will not be supported.
    pub fn resolve_module_imports<'a, 'b>(
        context: v8::Local<'a, v8::Context>,
        specifier: v8::Local<'a, v8::String>,
        _import_assertions: v8::Local<'a, v8::FixedArray>,
        _referrer: v8::Local<'a, v8::Module>,
    ) -> Option<v8::Local<'b, v8::Module>>
    where
        'a: 'b,
    {
        let mut scope = &mut unsafe { v8::CallbackScope::new(context) };
        let s_spec = specifier.to_rust_string_lossy(scope);
        println!("internal synthetic import {:?}", s_spec,);
        None
    }

    pub fn create_module_from_source<'s>(
        mut scope: &mut v8::HandleScope<'s, v8::Context>,
        source: String,
    ) -> v8::Local<'s, v8::Module> {
        let module = ModuleProvider::create_module(
            &mut scope,
            &source,
            None,
            v8::script_compiler::CompileOptions::NoCompileOptions,
        );
        let _mresult =
            module.instantiate_module(&mut scope, ModuleProvider::resolve_module_imports);
        let _newmod = module.evaluate(&mut scope).unwrap();
        module
    }

    pub fn create_module<'s>(
        scope: &mut v8::HandleScope<'s, v8::Context>,
        source: &str,
        code_cache: Option<v8::UniqueRef<v8::CachedData>>,
        options: v8::script_compiler::CompileOptions,
    ) -> v8::Local<'s, v8::Module> {
        let source = v8::String::new(scope, source).unwrap();
        let resource_name = v8::String::new(scope, "<resource>").unwrap();
        let source_map_url = v8::undefined(scope);
        let script_origin = v8::ScriptOrigin::new(
            scope,
            resource_name.into(),
            0,
            0,
            false,
            0,
            source_map_url.into(),
            false,
            false,
            true,
        );
        let has_cache = code_cache.is_some();
        let source = match code_cache {
            Some(x) => {
                v8::script_compiler::Source::new_with_cached_data(source, Some(&script_origin), x)
            }
            None => v8::script_compiler::Source::new(source, Some(&script_origin)),
        };
        assert_eq!(source.get_cached_data().is_some(), has_cache);
        let module = v8::script_compiler::compile_module2(
            scope,
            source,
            options,
            v8::script_compiler::NoCacheReason::NoReason,
        )
        .unwrap();
        module
    }
}
