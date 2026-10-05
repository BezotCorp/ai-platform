use oxc_ast::ast::{JSXElement, JSXFragment, Program};
use oxc_ast_visit::Visit;

#[derive(Default)]
pub struct JsxDetector {
    found: bool,
}

impl JsxDetector {
    pub fn detect(program: &Program<'_>) -> bool {
        let mut detector = Self::default();
        detector.visit_program(program);
        detector.found
    }
}

impl<'a> Visit<'a> for JsxDetector {
    fn visit_jsx_element(&mut self, _element: &JSXElement<'a>) {
        self.found = true;
    }

    fn visit_jsx_fragment(&mut self, _fragment: &JSXFragment<'a>) {
        self.found = true;
    }
}
