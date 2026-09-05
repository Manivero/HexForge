;; WIT component whose `apply` never returns: fuel-exhaustion probe for the
;; Component Model path (`execute_component`), mirroring the core-module loop test.
;; Result is indirect (single i32 pointer), like all heap-typed lift results.
(component
  (core module $Core
    (memory (export "memory") 1)
    (func $realloc (export "realloc")
      (param i32 i32 i32 i32) (result i32)
      (i32.const 1024))
    (func (export "apply") (param i32 i32 i32 i32) (result i32)
      (loop $spin (br $spin))
      (i32.const 0))
  )

  (core instance $core (instantiate $Core))
  (alias core export $core "memory" (core memory $mem))
  (alias core export $core "realloc" (core func $realloc))

  (func $apply
    (param "input" (list u8)) (param "params" string)
    (result (result (list u8) (error string)))
    (canon lift (core func $core "apply")
      (memory $mem) (realloc $realloc) string-encoding=utf8))

  (instance $trans
    (export "apply" (func $apply)))
  (export "hexforge:plugin/transform" (instance $trans))
)
