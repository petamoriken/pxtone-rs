# f32.sqrt and f32.floor, which stable Rust cannot emit: the scalar intrinsics
# are nightly, and the stable SIMD forms need a splat and a lane extract around
# them. wasm-opt inlines these bodies, so each call site becomes the bare
# instruction.
#
# LLVM's wasm assembly rather than WAT, because that assembles to a linkable
# object file.

	.text

	.globl	lite_math_sqrt_f32
	.type	lite_math_sqrt_f32,@function
lite_math_sqrt_f32:
	.functype	lite_math_sqrt_f32 (f32) -> (f32)
	local.get	0
	f32.sqrt
	end_function

	.globl	lite_math_floor_f32
	.type	lite_math_floor_f32,@function
lite_math_floor_f32:
	.functype	lite_math_floor_f32 (f32) -> (f32)
	local.get	0
	f32.floor
	end_function
