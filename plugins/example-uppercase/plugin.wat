(module
  ;; HexForge plugin example: uppercases ASCII input in-place.
  ;; ABI: (func (export "transform") (param i32 i32) (result i32))
  ;;   param 0: input_ptr, param 1: input_len, result: output_len (output at same memory[0])
  (memory (export "memory") 1)
  (func (export "transform") (param i32 i32) (result i32)
    (local $i i32)
    (local $c i32)
    (local.set $i (i32.const 0))
    (block $exit
      (loop $loop
        (br_if $exit (i32.ge_u (local.get $i) (local.get 1)))
        (local.set $c (i32.load8_u (i32.add (local.get 0) (local.get $i))))
        (if (i32.and (i32.ge_u (local.get $c) (i32.const 97)) (i32.le_u (local.get $c) (i32.const 122)))
          (then
            (i32.store8 (i32.add (local.get 0) (local.get $i)) (i32.sub (local.get $c) (i32.const 32)))
          )
        )
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )
    (local.get 1)
  )
)
