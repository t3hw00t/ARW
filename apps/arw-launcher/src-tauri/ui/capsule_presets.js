(function registerCapsulePresets(){
  const strictCapsule = {
    id: 'capsule.strict-egress',
    version: '1',
    issued_at_ms: 0,
    issuer: 'local-admin',
    hop_ttl: 4,
    propagate: 'children',
    denies: [
      'net.http.*',
      'net.https.*',
      'net.tcp.*',
      'net:http',
      'net:https',
      'net:tcp',
      'net:domain:*',
      'net:host:*',
      'net:port:*'
    ],
    contracts: [],
    lease_duration_ms: 600000,
    renew_within_ms: 240000,
    signature: 'vl4YZkykkbCTtxmiwHEYvzBizTPbM65YCqd2cHmUDEWGDfLxspWcKH5Zk7vm5lnsSD3ixD2b1OjC++fC54DGAA=='
  };

  const presets = [
    {
      id: strictCapsule.id,
      label: 'Strict Egress',
      description: 'Blocks outbound network operations unless a scoped lease or egress allowlist grants access. Capsule refreshes automatically until disabled.',
      capsule: strictCapsule,
      serialized: JSON.stringify(strictCapsule)
    }
  ];

  if (!window.ARW_CAPSULE_PRESETS) {
    window.ARW_CAPSULE_PRESETS = [];
  }
  window.ARW_CAPSULE_PRESETS.push(...presets);
})();
