;; Component WITHOUT the `hexforge:plugin/transform` interface: the host must
;; fall back to manifest metadata (no hang, no panic) and refuse execution
;; with an explicit error instead of running something unexpected.
(component
  (core module $Core
    (func (export "noop"))
  )

  (core instance $core (instantiate $Core))

  (type $noop-ft (func))
  (func $noop (type $noop-ft) (canon lift (core func $core "noop")))

  (export "unrelated-fn" (func $noop))
)
