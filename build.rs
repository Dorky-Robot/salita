use std::process::Command;

fn main() {
    // Only rebuild CSS when template or CSS files change
    println!("cargo:rerun-if-changed=assets/css/input.css");
    println!("cargo:rerun-if-changed=templates/");

    // Try to run Tailwind CSS standalone CLI
    let status = Command::new("tailwindcss")
        .args([
            "-i",
            "assets/css/input.css",
            "-o",
            "assets/css/output.css",
            "--minify",
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=Tailwind CSS compiled successfully");
        }
        _ => {
            // Tailwind CLI not available — create a minimal fallback CSS
            println!("cargo:warning=Tailwind CLI not found, using fallback CSS");
            let fallback = r#"*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: system-ui, -apple-system, sans-serif; line-height: 1.6; color: #1c1917; background: #fafaf9; -webkit-font-smoothing: antialiased; }
.min-h-screen { min-height: 100vh; }
.mx-auto { margin-left: auto; margin-right: auto; }
.max-w-4xl { max-width: 56rem; }
.max-w-xl { max-width: 36rem; }
.max-w-md { max-width: 28rem; }
.px-4 { padding-left: 1rem; padding-right: 1rem; }
.py-3 { padding-top: 0.75rem; padding-bottom: 0.75rem; }
.py-6 { padding-top: 1.5rem; padding-bottom: 1.5rem; }
.py-8 { padding-top: 2rem; padding-bottom: 2rem; }
.py-16 { padding-top: 4rem; padding-bottom: 4rem; }
.p-6 { padding: 1.5rem; }
.mb-2 { margin-bottom: 0.5rem; }
.mb-4 { margin-bottom: 1rem; }
.mb-8 { margin-bottom: 2rem; }
.ml-2 { margin-left: 0.5rem; }
.ml-auto { margin-left: auto; }
.mt-1 { margin-top: 0.25rem; }
.mt-16 { margin-top: 4rem; }
.flex { display: flex; }
.inline-flex { display: inline-flex; }
.items-center { align-items: center; }
.justify-center { justify-content: center; }
.justify-between { justify-content: space-between; }
.gap-3 { gap: 0.75rem; }
.gap-4 { gap: 1rem; }
.text-center { text-align: center; }
.text-xs { font-size: 0.75rem; }
.text-sm { font-size: 0.875rem; }
.text-lg { font-size: 1.125rem; }
.text-xl { font-size: 1.25rem; }
.text-4xl { font-size: 2.25rem; }
.font-medium { font-weight: 500; }
.font-semibold { font-weight: 600; }
.font-bold { font-weight: 700; }
.text-white { color: #fff; }
.text-stone-400 { color: #a8a29e; }
.text-stone-500 { color: #78716c; }
.text-stone-600 { color: #57534e; }
.text-stone-700 { color: #44403c; }
.text-stone-900 { color: #1c1917; }
.bg-white { background-color: #fff; }
.bg-stone-50 { background-color: #fafaf9; }
.bg-stone-100 { background-color: #f5f5f4; }
.bg-stone-200 { background-color: #e7e5e4; }
.bg-stone-900 { background-color: #1c1917; }
.border { border: 1px solid; }
.border-b { border-bottom: 1px solid; }
.border-t { border-top: 1px solid; }
.border-stone-100 { border-color: #f5f5f4; }
.border-stone-200 { border-color: #e7e5e4; }
.border-stone-300 { border-color: #d6d3d1; }
.rounded-lg { border-radius: 0.5rem; }
.rounded-xl { border-radius: 0.75rem; }
.rounded-full { border-radius: 9999px; }
.shadow-sm { box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05); }
.whitespace-pre-wrap { white-space: pre-wrap; }
.flex-shrink-0 { flex-shrink: 0; }
.w-6 { width: 1.5rem; }
.w-8 { width: 2rem; }
.h-6 { height: 1.5rem; }
.h-8 { height: 2rem; }
a { color: inherit; text-decoration: none; }
a:hover { opacity: 0.8; }
.btn { display: inline-flex; align-items: center; justify-content: center; padding: 0.5rem 1rem; border-radius: 0.5rem; font-size: 0.875rem; font-weight: 500; transition: all 0.15s; cursor: pointer; text-decoration: none; }
.btn-primary { background: #1c1917; color: #fff; border: none; }
.btn-primary:hover { background: #44403c; }
.btn-secondary { background: #fff; color: #1c1917; border: 1px solid #d6d3d1; }
.btn-secondary:hover { background: #f5f5f4; }
.card { background: #fff; border-radius: 0.75rem; border: 1px solid #e7e5e4; padding: 1.5rem; box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05); }
"#;
            std::fs::create_dir_all("assets/css").ok();
            std::fs::write("assets/css/output.css", fallback).ok();
        }
    }
}
