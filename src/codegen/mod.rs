use crate::ast::Program;
use crate::type_checker::TypeChecker;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;

pub struct CodeGen<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    #[allow(dead_code)]
    pub type_checker: &'ctx TypeChecker,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(
        context: &'ctx Context,
        module_name: &str,
        type_checker: &'ctx TypeChecker,
    ) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self {
            context,
            module,
            builder,
            type_checker,
        }
    }

    pub fn generate(&self, _program: &Program) -> Result<(), String> {
        // TODO: Implement code generation
        // For now, we just create a dummy main function to verify LLVM setup
        let i64_type = self.context.i64_type();
        let fn_type = i64_type.fn_type(&[], false);
        let function = self.module.add_function("main", fn_type, None);
        let basic_block = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(basic_block);

        let ret_val = i64_type.const_int(0, false);
        self.builder.build_return(Some(&ret_val)).unwrap();
        
        Ok(())
    }
}
