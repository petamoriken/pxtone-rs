# f64.sqrt, f64.floor and f32.floor, which stable Rust cannot emit: the scalar intrinsics
# are nightly, and the stable SIMD forms need a splat and a lane extract around
# them. wasm-opt inlines these bodies, so each call site becomes the bare
# instruction.
#
# LLVM's wasm assembly rather than WAT, because that assembles to a linkable
# object file.

	.text

	.globl	lite_math_sqrt
	.type	lite_math_sqrt,@function
lite_math_sqrt:
	.functype	lite_math_sqrt (f64) -> (f64)
	local.get	0
	f64.sqrt
	end_function

	.globl	lite_math_floor
	.type	lite_math_floor,@function
lite_math_floor:
	.functype	lite_math_floor (f64) -> (f64)
	local.get	0
	f64.floor
	end_function

	.globl	lite_math_floorf
	.type	lite_math_floorf,@function
lite_math_floorf:
	.functype	lite_math_floorf (f32) -> (f32)
	local.get	0
	f32.floor
	end_function
