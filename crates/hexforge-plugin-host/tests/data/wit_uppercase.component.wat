;; Real WIT component for `hexforge:plugin/transform` (WIT hexforge:plugin@0.1.0).
;; Test-only: lets the host exercise the Component Model path
;; (`execute_component` / `try_get_wit_metadata`) instead of the legacy
;; core-module fallback.
;;
;; Canonical ABI notes (wasmparser MAX_FLAT_FUNC_RESULTS = 1):
;; - heap-typed RESULTS are returned indirectly: the core func returns a
;;   single i32 pointer to a result area holding the flattened value;
;; - heap-typed PARAMS are still passed directly as (ptr, len) pairs
;;   (up to 16 flattened params).
(component
  (core module $Core
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 4096))

    ;; Static strings.
    (data (i32.const 1024) "comp.uppercase")      ;; 14 bytes @1024
    (data (i32.const 1040) "1.0.0")               ;; 5 bytes @1040
    (data (i32.const 1048) "Comp Uppercase")      ;; 14 bytes @1048
    (data (i32.const 1064) "Text")                ;; 4 bytes @1064
    (data (i32.const 1072) "{}")                  ;; 2 bytes @1072
    (data (i32.const 1080) "full-buffer")         ;; 11 bytes @1080

    ;; Static result areas holding flattened (ptr, len) pairs (little-endian).
    (data (i32.const 2048) "\00\04\00\00\0e\00\00\00") ;; id: (1024, 14)
    (data (i32.const 2064) "\10\04\00\00\05\00\00\00") ;; version: (1040, 5)
    (data (i32.const 2080) "\18\04\00\00\0e\00\00\00") ;; display: (1048, 14)
    (data (i32.const 2096) "\28\04\00\00\04\00\00\00") ;; category: (1064, 4)
    (data (i32.const 2112) "\30\04\00\00\02\00\00\00") ;; schema: (1072, 2)
    ;; caps tuple (bool, bool, string): deterministic=true, streamable=false.
    (data (i32.const 2128)
      "\01\00\00\00\00\00\00\00\38\04\00\00\0b\00\00\00") ;; (1, 0, 1080, 11)

    (func $realloc (export "realloc")
      (param $old i32) (param $old_size i32) (param $align i32) (param $new_size i32)
      (result i32)
      (local $ptr i32)
      ;; align heap up (align >= 1 from canonical ABI calls)
      (global.set $heap
        (i32.and
          (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
          (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))
      (local.set $ptr (global.get $heap))
      (global.set $heap (i32.add (global.get $heap) (local.get $new_size)))
      (local.get $ptr))

    ;; Each getter returns ONE i32: pointer to its static result area.
    (func (export "get-id") (result i32) (i32.const 2048))
    (func (export "get-version") (result i32) (i32.const 2064))
    (func (export "get-display-name") (result i32) (i32.const 2080))
    (func (export "get-category") (result i32) (i32.const 2096))
    (func (export "get-params-schema") (result i32) (i32.const 2112))
    (func (export "get-capabilities") (result i32) (i32.const 2128))

    ;; apply(input_ptr, input_len, params_ptr, params_len) -> result-area ptr
    ;; holding (discriminant, out_ptr, out_len) with 0 == ok.
    (func (export "apply") (param i32 i32 i32 i32) (result i32)
      (local $out i32) (local $i i32) (local $c i32) (local $area i32)
      (local.set $out
        (call $realloc (i32.const 0) (i32.const 0) (i32.const 1) (local.get 1)))
      (local.set $i (i32.const 0))
      (block $exit
        (loop $loop
          (br_if $exit (i32.ge_u (local.get $i) (local.get 1)))
          (local.set $c (i32.load8_u (i32.add (local.get 0) (local.get $i))))
          (if (i32.and
                (i32.ge_u (local.get $c) (i32.const 97))
                (i32.le_u (local.get $c) (i32.const 122)))
            (then (local.set $c (i32.sub (local.get $c) (i32.const 32)))))
          (i32.store8 (i32.add (local.get $out) (local.get $i)) (local.get $c))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $loop)))
      (local.set $area
        (call $realloc (i32.const 0) (i32.const 0) (i32.const 4) (i32.const 12)))
      (i32.store (local.get $area) (i32.const 0))
      (i32.store offset=4 (local.get $area) (local.get $out))
      (i32.store offset=8 (local.get $area) (local.get 1))
      (local.get $area))
  )

  (core instance $core (instantiate $Core))
  (alias core export $core "memory" (core memory $mem))
  (alias core export $core "realloc" (core func $realloc))

  (func $get-id (result string)
    (canon lift (core func $core "get-id")
      (memory $mem) (realloc $realloc) string-encoding=utf8))
  (func $get-version (result string)
    (canon lift (core func $core "get-version")
      (memory $mem) (realloc $realloc) string-encoding=utf8))
  (func $get-display-name (result string)
    (canon lift (core func $core "get-display-name")
      (memory $mem) (realloc $realloc) string-encoding=utf8))
  (func $get-category (result string)
    (canon lift (core func $core "get-category")
      (memory $mem) (realloc $realloc) string-encoding=utf8))
  (func $get-params-schema (result string)
    (canon lift (core func $core "get-params-schema")
      (memory $mem) (realloc $realloc) string-encoding=utf8))
  (func $get-capabilities (result (tuple bool bool string))
    (canon lift (core func $core "get-capabilities")
      (memory $mem) (realloc $realloc) string-encoding=utf8))
  (func $apply
    (param "input" (list u8)) (param "params" string)
    (result (result (list u8) (error string)))
    (canon lift (core func $core "apply")
      (memory $mem) (realloc $realloc) string-encoding=utf8))

  ;; Interface exports are instances: wasmtime resolves
  ;; `hexforge:plugin/transform/get-id` as instance export + func name.
  (instance $trans
    (export "get-id" (func $get-id))
    (export "get-version" (func $get-version))
    (export "get-display-name" (func $get-display-name))
    (export "get-category" (func $get-category))
    (export "get-params-schema" (func $get-params-schema))
    (export "get-capabilities" (func $get-capabilities))
    (export "apply" (func $apply)))
  (export "hexforge:plugin/transform" (instance $trans))
)
