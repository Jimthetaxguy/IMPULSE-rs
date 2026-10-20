---
schema: quirewiki-page@1
id: concept.code.impulse-rs
type: concept
title: impulse-rs
status: draft
confidence: high
visibility: public
freshness:
  class: evolving
  review_after: "2026-11-27"
sources:
  - uri: Cargo.toml
    id: source.68b5adcb475d
    hash: "blake3:0ae685b6830d88b61dc428968dfdc302360c3ab87f5aeb8a2593a37a53d6f578"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: README.md
    id: source.0a096ba47097
    hash: "blake3:4e1e0ebbf36a3ad141653547fe6976c9aa7105929c22d30d885c128b8fd6e9b4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/agent/coordinator.rs
    id: source.7384604da0f5
    hash: "blake3:235cc8fdd6dca92c112443908e633f3b76b05774dbbbace5433f7e14159de016"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/agent/harness.rs
    id: source.cf3d53ae512f
    hash: "blake3:794f59e4c53b1337966fcafcbe760756d659bab4bbb478f3586c5bd04518112c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/agent/mod.rs
    id: source.0ab2bc446ca3
    hash: "blake3:796b10bcf55a75155f1eff1d4984c6dd85deb5a56468737def426a11b95c345e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/agent/prompts.rs
    id: source.8ba8e818b809
    hash: "blake3:d6e645af8439948e8d15db768c1b3dce33b3d749e28cbc393dc2f9739626c12a"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/agent/step_model.rs
    id: source.3888baa85546
    hash: "blake3:9fd762e479ed093c044e31b4b326106c16743fbecac07a789e9e0028d4410d63"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/agent_discovery/mod.rs
    id: source.81b7d123db9d
    hash: "blake3:f16c58973bd8bc4c9fd0485d12a4253ed9662611c947a06daf7a5a5f6050f30d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/basis.rs
    id: source.4467a9c77487
    hash: "blake3:30d452cdc927d8ae1fde0b8225db9f201478cde2c2efec0ca774a54041d33509"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/bin/ion.rs
    id: source.1d7f304334fc
    hash: "blake3:ce62398170e558acc147d6564868eb8048d6917a524821d1560c76e5abe79a0a"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/branding.rs
    id: source.3fb626265ef0
    hash: "blake3:3e1f737082b134220dd4f56145dcd365dbb18ee6c0babc333ca2ad7d6ea6bf6e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/build_hygiene/clean_all.rs
    id: source.3396fcdeba9a
    hash: "blake3:8ad7a8957870029710a7ca9640f191fe3ff8dfd7f3c322becdb57c1c6c3094b4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/build_hygiene/discovery.rs
    id: source.ec54e80732e5
    hash: "blake3:9c38ac1611a3da11a7dd0a4cb6c204d3971f69a36cce39a814b852892c489bd0"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/build_hygiene/measurement.rs
    id: source.3d252327949d
    hash: "blake3:207c16feaaa3923470ee3c104f973b06c727dfe153883fe0fa209ac07bc6099b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/build_hygiene/mod.rs
    id: source.ccf2a131a04b
    hash: "blake3:967ff9ec000f18eb75b4248e49d06434c630925be1b024cd58140ab19a9913ba"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/build_hygiene/native.rs
    id: source.e5435220e495
    hash: "blake3:918cffe22c606af290d9da71b49cdd42fe5da3fc081d463e6e1bd4374cf0bd29"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/build_hygiene/sccache.rs
    id: source.652b0383baf3
    hash: "blake3:f17e3cbf40636603a66bde33992dab81aa3183e519676f5a3b4aac8035a67f31"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/build_hygiene/sweep.rs
    id: source.b3d7b705096e
    hash: "blake3:2eca206ff230c48594a74846d22a9a1146eccfb5a4a205637282ec7587f77a55"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/build_hygiene/tests.rs
    id: source.8b8aa2c46702
    hash: "blake3:2ab29f20430b1725ba1a4910696210307c1f7787d3d882a07145a0679b44dac9"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/build_hygiene/wipe.rs
    id: source.ba46bea51bb8
    hash: "blake3:d03e4545418751ee2210839c24accb51d89e2e8877dff3fd295ddfeeff619f82"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/cli.rs
    id: source.7e2889c7dbaf
    hash: "blake3:e9cd6b4cf63d709f34ab7b3f8633399095fa6ecef3976710425796562fe113d2"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/client/mod.rs
    id: source.f61b9271b33c
    hash: "blake3:9f93c6c499c19165c42e1739c6346eaa13ce72bb771413f156af4093b1f90bf3"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/context_lifecycle/detector.rs
    id: source.b4a53b939e70
    hash: "blake3:9a6d8820f58fdd142eddcf696e19e46bc338f40bcf9e05b5662bfab4ee1e8cfe"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/context_lifecycle/extractor.rs
    id: source.22615b5bb014
    hash: "blake3:cb7f85c8176d02f1fdfbd5a89b2147ad19e349b0bd10b272e578b7a517293d43"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/context_lifecycle/injector.rs
    id: source.7e3c6604ce03
    hash: "blake3:d3d9ba86b6fbee807ec14537cdd953108c12218b8dd5b0fb44aea78136691b54"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/context_lifecycle/intent.rs
    id: source.f2f79996a0bf
    hash: "blake3:345e55f2eb5c20d95041c134405068cfd61ad9b694b9dea57b4e7d276976d25e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/context_lifecycle/mod.rs
    id: source.11f6b5934d4d
    hash: "blake3:049a098cc41a75b89f7b2013396f3ee6161d56b7d344d81173d1503f09d5cb36"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/context_lifecycle/monitor.rs
    id: source.8eedad3eb4f8
    hash: "blake3:f9238a81c84bbd646e3f1516393979030fe017245b1c2e89f739c87427fad9ae"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/context_lifecycle/parser.rs
    id: source.6915b0142ce2
    hash: "blake3:cf783beee219f936a37f9573152a6f250d289ce320991830b3c8bd3e66e2e2e4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/context_lifecycle/templates.rs
    id: source.9ed05d788b69
    hash: "blake3:4fc248bf38455b35757ba5eea2924d0059dfaa16ecca4abfc54b178a983694df"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/context_lifecycle/types.rs
    id: source.e1cc2fca276b
    hash: "blake3:deead070a78db6f5d2334671f6b486187a11dfc27432d9ee067aa9758db7c01e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/credentials/cli_proxy.rs
    id: source.58ad30a5fb68
    hash: "blake3:44045243fcf1854974a1286e09027c3273157ea68a7eeda840af0bae13c394bd"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/credentials/keychain.rs
    id: source.0c3aa7f35bc2
    hash: "blake3:247be7211b2a1046bd6084de86b383a45312b9c69a8042b71b4040e0fb146f77"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/credentials/mod.rs
    id: source.1e0de7ec4682
    hash: "blake3:828501dff665439b3bb93a1b765c14e92605397111f6c3fc074d97a2ce741f1a"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/credentials/socket.rs
    id: source.e57208c7bde6
    hash: "blake3:c8bfc174f2a6a46403bbbd20bf1b5a480394ff90c3ccdf788af31d5656adb914"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/daemon/handlers.rs
    id: source.3667bc3cb20c
    hash: "blake3:a14a52a871002da5e005f50c5130cfe8e29af7ef5efca1033561776d8e216a12"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/daemon/mod.rs
    id: source.e699d7708184
    hash: "blake3:fcaa218d9e6ffcfc9d2f30bd8119f200ddef0793f40b9ecf9258f90f6e6e917d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/daemon/protocol.rs
    id: source.f21a161e050a
    hash: "blake3:1761c73c1d7e6026bb7e39ea260e30a4ad004e271b323639d788c9490d3cb8dd"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/daemon/tests.rs
    id: source.722a568dd90e
    hash: "blake3:e1ba9aa17546fc55feded2bd2ff2ba5e5ca47a6ee2cadd8b4449d93f2611feec"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/delegation/detector.rs
    id: source.f28dbdd93b82
    hash: "blake3:ce7fcf14054f662cd91c3c64d22d47f178781ec6a3cce4fb9cc16ca80748edf7"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/delegation/mod.rs
    id: source.290d3f08e789
    hash: "blake3:e92a6814ab4f4deb4309d904843b14fcdc5b0fd154781233cac26df8f11d242a"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/delegation/tracker.rs
    id: source.c6cc3a00da4d
    hash: "blake3:9f431d6ff75dd57de4d730099776af3cc1ad1590670d411a34bac3b5c99126f4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/delegation/types.rs
    id: source.090eec52f687
    hash: "blake3:453dac30695cedefe2f1fcd5fd86edda245770dd0d45d3a2cab7812e4ad10a3c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/docs/cache.rs
    id: source.88dc304f5c2a
    hash: "blake3:2f37d2808d3ab1eee4dbf5eb0a259983796a0af728cf1478aab89d659ae5baec"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/docs/fetch.rs
    id: source.a3e72f070f12
    hash: "blake3:e44ac0804288752e1ff17131c735f28e155390128da98045326eafae14c15e4b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/docs/mod.rs
    id: source.780714a818e1
    hash: "blake3:e185788ebdb85d0995a562fa52e8a9559ca0879a872dca801f5287ba2f5c0a5f"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/docs/models.rs
    id: source.ef27f77a5c60
    hash: "blake3:32fec6493a549cac2f5be60a1ac7d0425f0917767e4572597eef7bb883160f80"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/envelope.rs
    id: source.eaf07ae29509
    hash: "blake3:e8b0433fef8379163db12bf85d9aced18a8c21c46e50d34fdf0cf156b433aedb"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/error.rs
    id: source.6885179cac03
    hash: "blake3:1727a748d9dbb1a790687d733ae893c6fe402dfb4d7a54fb4c673738d17f75c9"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/governed_producers.rs
    id: source.27cc14a4e7c9
    hash: "blake3:931d37f5ffdb8b804994fc431765f271b7cbb7ebe9f04186da98f0c7ff6fb52c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/guardrail/config.rs
    id: source.f94cc8def746
    hash: "blake3:b3c04f5a482384454fb4fbec9234aa28cfb7d23847c0285ed5de0d054d073bcc"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/guardrail/defaults.rs
    id: source.f4d06bcc5131
    hash: "blake3:4a75ad3f37abe34c662e7e2319d34de25cec0278956799ee750df48fb14aca3d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/guardrail/engine.rs
    id: source.e106268caab6
    hash: "blake3:0af10ee5de9ef379afbac45aa919c5809fb8faf17810c2daf7d089a241811c85"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/guardrail/mod.rs
    id: source.d23f053b8b5e
    hash: "blake3:cc35abb145f0a95ccae405dedae7c52cb72054d649eb71cc8e28d96589be4674"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/guardrail/types.rs
    id: source.718973897d9d
    hash: "blake3:93aea99e981e16894c09fe0df2e23dbda152919c0c769063f071f4dcafe7a92b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/agent.rs
    id: source.6f92b744dfca
    hash: "blake3:00d4b04bef2dbcec189d4a6a3a9ba2f03aeb44bbc30fd574454744caa8b29879"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/build.rs
    id: source.6d19c04e8d1f
    hash: "blake3:646b4025f44278a510f88a7c441a0c3612c554f6a6f1dc8bc45b2217a9749aee"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/common.rs
    id: source.5a704d642484
    hash: "blake3:24fa010fac97232649f0990a1c55050c8a37100ed03c7687b2114c43b5e7bd80"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/config.rs
    id: source.f086bb7e7377
    hash: "blake3:8ddd6217924c415d817f157669836338e93d3c84dea10848ea13bccaa88f4601"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/daemon_dispatch.rs
    id: source.49b9923f8bd9
    hash: "blake3:4474f42d1b3a95ebc7d28108c9377cc9b4e1e774e3475a94112604089ad969af"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/describe.rs
    id: source.fa855c587aab
    hash: "blake3:bde1c42fb4ce598609b74524c673da6b3fb670c802604ea5cf92cbcf7c3a4490"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/direct_dispatch.rs
    id: source.a4a979d55196
    hash: "blake3:fb93cee5c52e3ccb39b4108eabd6c0502de3c9565571b05ff9691b331c7ee2b5"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/guard.rs
    id: source.2018cebd96a2
    hash: "blake3:1ecc163aaeb7305bb627ccc7d1f21ec6acf24ed7835ef98247e4cd3833a5c4a6"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/injection_handlers.rs
    id: source.3d401df02663
    hash: "blake3:a27cac405ee8010748fca77ae74ebbcc9c91966f343908047a6919fbdc799ad6"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/ion.rs
    id: source.60d693c6d822
    hash: "blake3:b9d8d0b8c84f7324dcc3dd3dff80a9d771a78cbe8e0f2f24e6ce103716b1408d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/memory.rs
    id: source.558bf209a520
    hash: "blake3:84f3c3b5cbec9eaed41beb55eb38ad1ee7a69c4693e082d18c1be1a582064cae"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/mod.rs
    id: source.b7183ad543a3
    hash: "blake3:400490aa4a93f27d9eb4e940c495eee812588b003b34cb3d02f52094a91aef5c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/office.rs
    id: source.bbc0036e56dd
    hash: "blake3:968e8f3d87a502945baada0273f2e598454e9209432da130f4e0c9cb5bfa21f5"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/plugin_handlers.rs
    id: source.ea5b060fad41
    hash: "blake3:8dfa3231d4541e99f61076c4b580b8738ffbb0f8af356f93cdabcebec078b993"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/retrieval.rs
    id: source.adb20f4c2e2d
    hash: "blake3:649b558cc11f38465fbf2e77f26ab2cd107a93ec456ff86cfe8ff2f3dd6330a1"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/semantic_diff_handlers.rs
    id: source.f27733a64717
    hash: "blake3:90f017e45f3ea5da6a07c4bf765b3fe549aa73e39ad853014efd77fe5d1d2097"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/session.rs
    id: source.80dafe647c6d
    hash: "blake3:cecf027ee6f06f83ad714da5fcffa23ef5f35bb0790231818cb14bfea19bb18b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/stewardship_handlers.rs
    id: source.38d5126e7286
    hash: "blake3:0db098c2ea01f524f3cbe1abd0387bd483d2a458164100a85e0668eb93954633"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/system.rs
    id: source.d69ec3b338cd
    hash: "blake3:0b79ada32e6adc8ecf6ecfbb58a33189ec5eb214fe57a79d54b9508b9527f9bb"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/tooling_handlers.rs
    id: source.2b71592fc82f
    hash: "blake3:6f184ac2e65c44b675f438cd9a2ef60b5f32343b70e6ba8f78b5d4ce488b3145"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/handlers/voice_handlers.rs
    id: source.041b122138be
    hash: "blake3:d8a8f351aad28c8d4ed06e295f127a9ddbc4975fb500f36bf926afb3ae0fe057"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/injection/engine.rs
    id: source.86a08236851d
    hash: "blake3:e8219b774f6e84ce07d1dcf0dcc3b12b706b4351ad89d9455317a4b32ea7fc75"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/injection/mod.rs
    id: source.b76dc87ac881
    hash: "blake3:7cfe0ed9d2f87d8d2a24973eadbbcad6119342d2b00e9c7031a75ca6c2f57083"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/injection/staging.rs
    id: source.2dd2bec0ce54
    hash: "blake3:bc94d4481265cb65ff34ec784e4af3b2f2d5e0e0f1f3b2fb8676cc60bf1b4fce"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/injection/types.rs
    id: source.93edf05410e1
    hash: "blake3:53af9dd17dc9e7485e4067ec4ab0514c7b2dd8b6e5c63936b00dc84a41e2b03c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/integration_tests.rs
    id: source.0524f1dee070
    hash: "blake3:2c1201d31028d86a6b27f2e9845853b6635ee70d9e7434cb5192cc61fa533c1d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ion_repl/chat.rs
    id: source.d9e2947e29b1
    hash: "blake3:0d95161b3c0adc4df52a4031babac496612e02ea503db83276f58fac09b3e8fb"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ion_repl/history.rs
    id: source.a5188284e885
    hash: "blake3:20fff72e955204ea6e936633ecca787b03e9d02e73a3a1a2817ef27b5f057379"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ion_repl/mod.rs
    id: source.9e6656e323ad
    hash: "blake3:d4cb79541b83f027eff1e72e1039174cce850fe09b349dfd6f69d17618ad5476"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ion_repl/registry.rs
    id: source.d8fb3ca11ee4
    hash: "blake3:63127375984917e7de3ff651d298627efac7b7d28ea7c4e1cbb5b57441a55931"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ion_repl/router.rs
    id: source.d2eafbca7c2c
    hash: "blake3:2fa8dfbbd64aa12f50c713207f527cc4fb8f5c9f7cdbea5df40f4bc808ff429e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ion_repl/tool_bridge.rs
    id: source.4b09168a98a7
    hash: "blake3:13b9d0e234c36e22aa3ce69f2d94669b77f6df684748df54885e1cb2a331d4d8"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ion_repl/tool_claim.rs
    id: source.121058c4b7da
    hash: "blake3:85466129154fdd8bad3cc8233a434f4d2d1c7d45430fdeea2d278a0f06662f0c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ion_repl/tool_verify.rs
    id: source.a4298c7dfcc9
    hash: "blake3:a705432269cd1aa1676b08b98550ee89b4c976914b7770a1a7b3894cff34534e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ion_repl/tools.rs
    id: source.695fbf6802be
    hash: "blake3:43fec23ab918ad56bc32fe4c982109b3a60ddaf23ebc5e0ce41bed9ce4a1e2cf"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/lib.rs
    id: source.e3f85dfb5b61
    hash: "blake3:371c9de39d850098a41f73a9e0550888ea21493184ae70e43f670697f1a2dd1a"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/llm_backends/anthropic.rs
    id: source.15f5db715fa9
    hash: "blake3:98f52ab025ac78a9f8ece6e8720fb49687c6f93e26b136906d55bea79940ea61"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/llm_backends/mod.rs
    id: source.6160d3d8abe6
    hash: "blake3:02c8129b59148d482aebecb9bf304329edbdb64104b86e57a677dd0fcc306272"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/main.rs
    id: source.6d9bc68d5fc7
    hash: "blake3:3edcddb42025a72f0b88b900bf054df95075a3214760238d4403260c48f3f1cd"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/mcp/mod.rs
    id: source.d9d933c75fdc
    hash: "blake3:ca797b454565fe480da88a1dfecb857bf85b958ac64aeb1b65419c9c4b46d0f8"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/mcp/server.rs
    id: source.6f6f88c4d931
    hash: "blake3:d2ef71fab57a23b03d841bef1c2d054e5158cc0120194ad56feff8debe868541"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/memory/mod.rs
    id: source.9c87599571c5
    hash: "blake3:b4d254e2be7f7e888bb7577d22cdc9a96cfebb96034f723b3b5801fdd3da316d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/monty/datafusion.rs
    id: source.650beefbf907
    hash: "blake3:49a93857e4d27bd87510f14bf361531fa21c3820a8d0f463d8e81b4ff0904797"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/monty/kdb.rs
    id: source.c0be68b49440
    hash: "blake3:43a02985952bbe42d1d514d57c29b8e7e7d425b0ab39931c8d27992804f3776e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/monty/mod.rs
    id: source.2c8156ed8849
    hash: "blake3:b3c799a1d2890800c7b328de0d09ec90727785539094de22cb5b4996859857cf"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/monty/python.rs
    id: source.a5a1b9c152bc
    hash: "blake3:d922604e5a0b09420f3661e81e3a854d4de58a1ecf5bcaeca721dc43ce4fcece"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/monty/routing.rs
    id: source.b15131e20091
    hash: "blake3:79805b80dc8d87f1de10bec072e5ac607ca2d08f4b8f1749275200b68bd3e8cd"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/monty/swarm.rs
    id: source.4c5c87b67cfc
    hash: "blake3:084ba6368351f946c733f7ed8500469804e5620dc96109ad2ca3ec6a26a27f1c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/notification/mod.rs
    id: source.f16a8fd990a4
    hash: "blake3:b1bfbc51d393dbaf93c636e1e465c8b9a42ce2c0d305264f4f13057b6a0fdca2"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/office/excel.rs
    id: source.a925ac0a0fe8
    hash: "blake3:fd42a173be84197aa7d057b9590ae8bb1b60ec487909a9feab2acb25a8c271e1"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/office/extraction.rs
    id: source.e52fa5888fa4
    hash: "blake3:d46a7b5539a066eead3284936e9ac88020f5c80901666489cba1a7ad3c9d5693"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/office/mod.rs
    id: source.0dac06f82c4d
    hash: "blake3:37677303b505f08e059a033dbe13c32141f39ccf3365f205934a93f8f9b88155"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/office/word.rs
    id: source.8a443f2d8e08
    hash: "blake3:24abdee1923794a5dfa6b44e98cc3512e6f1a546e2f3735cd17db53f587a36d9"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ops_workbench.rs
    id: source.62fe64a8f000
    hash: "blake3:0f5ada06442ceba7ead75d6ad082e024ae43a2a407e72b4101492144bb3ffb19"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/orchestration/mod.rs
    id: source.9104f4a3a505
    hash: "blake3:5909054a62991c030da9d387940a2c7580fb060baffbda112679966674e98c0e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/plugin/action.rs
    id: source.18bcb35db850
    hash: "blake3:554d1be0914089906c2ebe053c5885ea1000c05f01279203b340eb95875668db"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/plugin/context.rs
    id: source.beb11d2bae81
    hash: "blake3:a75e2fd60ed164c57a5ae6c491e943e13dbd253cea7cde031e2ac026b26d840e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/plugin/mod.rs
    id: source.3cd924e4dab1
    hash: "blake3:0156cb2b45a31daa3867997461fb02fbdd8b09ab26814775038e2d3385f0b6d8"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/plugin/registry.rs
    id: source.3419ced2c064
    hash: "blake3:72228475eb14f69ee37211fee6bb0a5ed2fff1404728ef1ee628918de3165f96"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/process_group.rs
    id: source.49ccaac8d802
    hash: "blake3:7dcca3ecabed32e585642a834321c1f1af66983d46a18fdefd53219c4d80c035"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/process_util.rs
    id: source.efb60f7d6cfa
    hash: "blake3:358edb49cf970e05c338d5cd475cb1d470a8cebae126c025dfd6f82773c097a5"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/retrieval/embedding.rs
    id: source.7b806e013917
    hash: "blake3:d85b1fad2b9bad8fa2c38ab0cc8f0c31503d407b56bdea8f6e712463ab651427"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/retrieval/fuzzy.rs
    id: source.ee843a599984
    hash: "blake3:8fba7dac464eef5363e77743e590c8277b916bdbb5181fb944e9a3a1c73efa4b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/retrieval/indexer.rs
    id: source.11a41593e2dd
    hash: "blake3:67cc65ab6034561cf01d4c9f767206c950df57bccba37c481ff2e1bb846993ae"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/retrieval/mod.rs
    id: source.bb1c86a8ca6b
    hash: "blake3:087d1085daa062dafea30e9aeb590538ba58f765ba55bc3504046eeffaeface0"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/retrieval/pageindex.rs
    id: source.ac3b24f3c744
    hash: "blake3:26a90882acef48914e5c8e26d5fce9caf54a95403003f4d9105ffbf6c17e1a37"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/retrieval/query.rs
    id: source.8c81a34bb1fe
    hash: "blake3:5317f2f527870f4d3444f3718fde9c24c0d9770c150acd38cf1ee9ec4401de55"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/retrieval/store.rs
    id: source.a254e8928d58
    hash: "blake3:8bb821aaede2b540424990c6b5043197d972b22ed78411711aa83d8ed47b9b9e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/retrieval/types.rs
    id: source.eebd3a81d9ee
    hash: "blake3:dbba2c908f13350c7877fb8e1fb6cf38ca6a6ae88808416bc885212a653a79d9"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/semantic_diff/mod.rs
    id: source.2c06ffe19d47
    hash: "blake3:942eb755485d9a36f7a9b1df860dff9582774afca0513b3f455b5db6b901f094"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/semantic_diff/runner.rs
    id: source.afd66dc918ea
    hash: "blake3:e42180d0f2066ce70d70c461ebaf190cfacc1af933e1fb85fa5fb82d3ace8670"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/semantic_diff/types.rs
    id: source.d56cebd11030
    hash: "blake3:2499435b4599daf2379f67d0421c18d08277b56b97bdb4d43167c890b9bbc4db"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/settlement.rs
    id: source.730aa00aa5fc
    hash: "blake3:c0ae084ac2a778f5727d66e9c7f5f05f5c832e6f05eeba71d346cdbfe6b953dd"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/state/config.rs
    id: source.c3e0d3fffcf6
    hash: "blake3:0a5a5044ab84de4ca446c3bac5e82d23a4a579acf59e3a37425082e2a2aa797b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/state/config_keys/mod.rs
    id: source.548ec9801502
    hash: "blake3:1fa238a98221d730c7af0187cc1c14caa728f698bc7cab0099bd7d78449443bb"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/state/config_keys/rules.rs
    id: source.998476021f39
    hash: "blake3:10f47140922c234898552d5e548f1d1f11b79c0ca6a86ed4a99ae2b148cdaaa6"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/state/config_keys/tests.rs
    id: source.804cb08ef0a9
    hash: "blake3:298829fe95ea220da80f3c7d09ec63378003879167744357709c99d2bc8a54c4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/state/governed_task.rs
    id: source.973c4f9d6021
    hash: "blake3:9988dbb368a5f391b4a0dd11021e14080385fcf9fb9223b88175c2bb1e7250b0"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/state/memory_candidate.rs
    id: source.efade6b7e591
    hash: "blake3:1a70c107342b7b82b96dc7aed64a856e819ba542208f2a6be31dbdc82834662e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/state/mod.rs
    id: source.ff4c1fa45b7f
    hash: "blake3:731ba711e4ede5ae60b7433d27a25bc7d53ea91aa646af4b2feca1b630285217"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/state/persistence.rs
    id: source.e8419dcf6281
    hash: "blake3:6a66bec4140a97965319b6fa2da13496f4aa72acf6e9420433c5d2d7f38bec4e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/state/session.rs
    id: source.8923a1e3620a
    hash: "blake3:ef48582829fab385ca9c1bdfda165913bff8b7b15f2886de341b3a6ff00b2752"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/stewardship/analyzer.rs
    id: source.2ae4b97be7b7
    hash: "blake3:9c06d3766652fb089ebd4ce741f357ef83931366c855e8a1560d5c8c01fabf10"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/stewardship/approval.rs
    id: source.af939cdde8a1
    hash: "blake3:e63ecc3674555c9e1276ea96c488ee5ec37f4b5fdd842c79317a6df0b6c80e0c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/stewardship/cleanup.rs
    id: source.12a4bb1192fb
    hash: "blake3:78d58cec8d9f31b39ec9c2a656780667b9e6a7efdc698fbc449911f11d531db7"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/stewardship/cross_project.rs
    id: source.07f6a9e788ee
    hash: "blake3:047938c79ddaf569e8dc9e5a0c4e9063451f7bc16262185f5ae3623731bed162"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/stewardship/mod.rs
    id: source.9d349223bd7d
    hash: "blake3:4c1e0660e8cf388609a0213ec3ce26369266f69f25db94417f17bc262f9282c6"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/stewardship/monitor.rs
    id: source.8cbda8678cbc
    hash: "blake3:fff0cdccbd9ee8c403ad85e08e547724b40a6482b81677a76ecce36d8808475d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/stewardship/types.rs
    id: source.da7a0a68eb54
    hash: "blake3:04dc331c1e79040305f90be481fdd973c2e0eaa5256af3b5329638e31cbc6330"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/storage/mod.rs
    id: source.3f3695a586fb
    hash: "blake3:22a07dc2b6b17747a5894ad4ab586267beb09850d781fefae274534bf370e0c2"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/test_support.rs
    id: source.b09860fa2fc1
    hash: "blake3:f0ed75a171118565f5c78a65ae8f7ccc67d3b65be8d58e2dec64a3097edaa62f"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/token_tracker/algorithm.rs
    id: source.8b9c9507f353
    hash: "blake3:d4ff620ccd6717bd3f973bba4d61085fdb0896cfaa2bc1d3032be321365b17f8"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/token_tracker/cross_platform.rs
    id: source.dec5b49456db
    hash: "blake3:46f8c4740a2ea735909e305f90556eec3b0d5f0e0ec2179395ae58faedb2e9ee"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/token_tracker/metrics.rs
    id: source.6c0b24f0e7c9
    hash: "blake3:afbb6d36af96a09ea2723e5fbc2301404cb280ca97a46433271e45662e5ee1c6"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/token_tracker/mod.rs
    id: source.0b44b800f40f
    hash: "blake3:0737dcdc6f92b0816167145f8452957f3820062308afb32277ef2a23c2ca9498"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/token_tracker/research.rs
    id: source.c7f7500138d5
    hash: "blake3:aab7b3e979ec17a2bb973657eb572dbd5fab54fc3c3cd3a7bced2c964ac76415"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/token_tracker/types.rs
    id: source.80984687ca11
    hash: "blake3:24fc9f4bb4a36854b3ac59f3df6b1bf8ea60ec7ab34d210104791e815b6faa62"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/bash_exec.rs
    id: source.0e27dfc19c2b
    hash: "blake3:68a51233755ff0fc9d35f8dd2d7ce474ea2f0bb1198ac189632da8dba03d664b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/benchmarker.rs
    id: source.75af2480978f
    hash: "blake3:3aee4e97a407c57fbfef407478826fdffaac95467d367d08e4998b91a733c4a8"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/build_health.rs
    id: source.30cc5933027b
    hash: "blake3:43024608c8721a0729c694405318ff928bb52d6c7b26ac50b2088f73f8cb87a3"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/calculator.rs
    id: source.a39e60e576c3
    hash: "blake3:7fe7b4f8841f935be1ba2d55ac4b752d8ca24bd174b6ec5d70affd183ef99084"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/config_get.rs
    id: source.8121070b3a0c
    hash: "blake3:4cb2a969049bc10fe92b3ac950ee1242a6195eb19d44ee060a589f27e7c501ba"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/document_extract.rs
    id: source.7506292fd3e3
    hash: "blake3:c57f262a05201b3f994d67de2ad6fe422b33ef09dff7f6dc85c2d31ad6b13f06"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/file_read.rs
    id: source.04972382bdfd
    hash: "blake3:462aff0037bfb22a471339f5a6723720cd7967a76aa8aa3469552db8a6109aa6"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/file_write.rs
    id: source.8ba530bc4903
    hash: "blake3:69248352480806a5a9f41f6f5e22acc6aedbffd1f9d26a72c66f005f7944e085"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/genome_read.rs
    id: source.5d34dffd0e4b
    hash: "blake3:6b0db84767079c74441358c157dc9b0a9ceeb3dfe914d0c6c22ab1b692228b9b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/health_check.rs
    id: source.fe73c0863916
    hash: "blake3:1902cce3f06b6a8d68b6c11afce09d20ee14fafcd0cb6a3bafbbc8181d7ad90c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/memory_search.rs
    id: source.f9af0feb71c9
    hash: "blake3:25f67af2a248b73d2d29c19e83dab6ff2027127b554950728632e32e59525039"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/mod.rs
    id: source.e106789da66a
    hash: "blake3:eeac001e1ed9aa77fd83f19dcdf248c9a9b917b70a6010ba4bb25572a5237da2"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/python_exec.rs
    id: source.9a95055cacbe
    hash: "blake3:2c7b01e6301f93a37aec0bd03aaf323f852ec3ceca0fd31b1bc73754b15e8c72"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/session_query.rs
    id: source.9000df2596f5
    hash: "blake3:1a1139cb3e93839c756c9f53cdd3520fe329fea49ac8db5712be06879f033263"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/steward_status.rs
    id: source.df45277cb1de
    hash: "blake3:6a0a18d349a14e978e47d91858e5519457109525f9cab0e62d490c931915820b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/builtin/system_info.rs
    id: source.6b86648e6906
    hash: "blake3:83af10f0bf4cdbfd7ce0015c543653a93fed6d821b5f8d104e352a9c9b93d8c3"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/document/document_parse.rs
    id: source.3fedd5c775d8
    hash: "blake3:ccefb6cb57fcbf8182830f1a1e6bf57000bf141fc7f70ea77112fe009942b274"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/document/excel_read.rs
    id: source.8e6d35fff5b4
    hash: "blake3:497ff86490d07f88ff144f0c691ac00b255bb7697e15d9ef81f7a2af0b780572"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/document/mod.rs
    id: source.e63978cee6cc
    hash: "blake3:252cf45801e05271222cb0156cec267b0a938b640d46291e9a0005c1e96351b8"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/document/word_read.rs
    id: source.59d536cd0147
    hash: "blake3:d03976a00d64cb0f744ed03e979df6029e689cc29b2d46050de96b7ab85b0f4a"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/env_scrub.rs
    id: source.a8b65bca5cc8
    hash: "blake3:31055eba8b3e51dfc2e1509219ec9bda689bfe7be982fe34118de4bb9ce5da96"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/error.rs
    id: source.092137f107a6
    hash: "blake3:8630c96f8f7fccc5bf1734ca8afaf3505b0c15da1760e62a0dd89ed7a9416474"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/executor.rs
    id: source.835810df6638
    hash: "blake3:72e389d5c907bbb2504208366040d8ca354bb7659f335ad2f23a64a8693f8f60"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/external.rs
    id: source.9a6a43d1cbf9
    hash: "blake3:0202d5407fb7fe57599f60bcf180383453f22cacb954c67cd2de6bcc76f35dc6"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/mod.rs
    id: source.f20797ff66ed
    hash: "blake3:3ee511caec8813ae89b2f9a2601c476ba70cd89a71bef3e99b545aa1a08020d3"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/registry.rs
    id: source.19ff72890568
    hash: "blake3:cb7423b390eb07bbd10a43ba5a2f2023bae57a570510a02b48e5a60478afbf4d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tooling/traits.rs
    id: source.7c8c176321c3
    hash: "blake3:66194fe153391213cb893d8a43b4603ba1a415939daccb42d9325e2d3ada84b4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tools/benchmark.rs
    id: source.a859ed91f6c9
    hash: "blake3:c819acbf96c082254aea819bb985f9f4f2fadaab35662398d1d8c1b026738ce4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tools/health.rs
    id: source.517f5be38050
    hash: "blake3:567a4296ea194f1da00537e7564a43ece79acad611849ac6530e48362c649f33"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tools/init.rs
    id: source.aae0821418f3
    hash: "blake3:cf3d22e05f75156a9bd975f4ef4a4dc0cc5c02b447b0c4f30e3fffd069de0882"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tools/list.rs
    id: source.d408cf6f9090
    hash: "blake3:ac7986d07158cb97102925511998e3ac64145097b03aad8320542b8808c3688b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tools/mod.rs
    id: source.a97528229533
    hash: "blake3:a8bdbb4546070b995ca716b175207de5acf7a058aeb94305b74624dde7139dbd"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tools/python.rs
    id: source.1a4d287f5264
    hash: "blake3:e316f932e3183b4f5cd990ab98621dd3de537a44b2daf90f8c3ae0d6278ce6a1"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tools/system.rs
    id: source.6d0789c7d62f
    hash: "blake3:4d92d3620c8c9a437f049c9cfe87333e5b9804f0c2f27b396d23f56bd98a441b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/tools/update.rs
    id: source.2ac2a70b934a
    hash: "blake3:fc9078c9c9d95ffbe145b0c248f95891894b13138bab9e64c01aefc50605f6f5"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/agent_terminal.rs
    id: source.b393c419519a
    hash: "blake3:0de8a262582e314365ec4db200b01469a4b4d63344cd49a53ffbd97bda7b8d46"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/lifecycle.rs
    id: source.3f27848151cb
    hash: "blake3:40f1c30f9da1828d8cce000cfbc0ae64ad53fd0591548400832c90ae0a54f245"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/mod.rs
    id: source.3d82e6801dfb
    hash: "blake3:1e6eb61334221e4639b8c4a95d7b2ad3d027fac39ac05ba09321b8600c481ebe"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/pane_manager.rs
    id: source.8df31a2376de
    hash: "blake3:c1828761577556c86e1d574b1e02f5b9249b62e6eebf451d135903ecf2d14a2a"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/render_content.rs
    id: source.0976ae928ea2
    hash: "blake3:d8848b1ede3f5f51a933f1c0958d712cf31c1b1279038a53f79b2e9bbc95a03f"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/render_dashboard.rs
    id: source.1ff5b87e8af2
    hash: "blake3:c75ff4c118be0d970c50d68659efef7345380a2869eebf6469f4e2a25627243f"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/render_menu.rs
    id: source.78c86f853eb7
    hash: "blake3:9b0d78c3297c677ba512562f3a39f461739abe7341b53d5f9a058810ac006e1c"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/render_status.rs
    id: source.c992ec454bc6
    hash: "blake3:c298bedc32cfcfa8e8a3f3667cf8d52a1eaca62b21fbbdee52e45bf17b023f49"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/render_tabs.rs
    id: source.26a84cdf69f1
    hash: "blake3:7c2c8fe2df793883a6fc14fd1fd39233a7ddb570f44c2b64da7452444cfde8b4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/runner.rs
    id: source.44026baa7c99
    hash: "blake3:42ba07b1ed0955d91708b04a3603cbb4bb377382759bc1034b0b0e4e273e9078"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/terminal_pane.rs
    id: source.b59b73bbd596
    hash: "blake3:c9be6ddab2ef6896ef95d27422dca7ff1b6d0aa14e7f905d384dc76454927e3b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/types.rs
    id: source.a385c6a39217
    hash: "blake3:aeefc65046d41012f450bf936be0cb534789ed45640277b1cd7591fe5fa4380d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/ui/visualization.rs
    id: source.db6235a3ee47
    hash: "blake3:c6fcdd8dcdf892e140e5ba0e383e44497a0725d4d5f52cdb4f24250abef1ebb4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/validate.rs
    id: source.6f1fd805a007
    hash: "blake3:06b1b2f31cb7be4c5c7a2e0c771abfddb3db1165195fbd4236b24a3eea8b9043"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/verify/mod.rs
    id: source.1d8c54464175
    hash: "blake3:05bac99fc8d84a61bea2d81b336e2279e2abacacf455afd0abf6939eb561fc3e"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/voice/adapter.rs
    id: source.a5da1a8d2efb
    hash: "blake3:2877ffec0a2e1d22fae516739444b36e26602f3b8ff8ce7d43bf97871b2018ab"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/voice/envelope.rs
    id: source.4a393e546686
    hash: "blake3:b8df56e02caa531e8c1bf35ac29cc6c5cc1d7828682c2e519ec530920b31fab5"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/voice/mod.rs
    id: source.1221074aebb2
    hash: "blake3:021f21dfe62caee5fe69b484e261ff89431d1f0beb9ca469ff3f37fd4c9d4f23"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/voice/policy.rs
    id: source.bac3f0854059
    hash: "blake3:5595b37decae65eb65337d40b83c1adf26bb831e592a1b772b880247cabe5927"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/voice/provider.rs
    id: source.471e1e1e858c
    hash: "blake3:300aa7a8ee833b901e3a3eb56f7c5d9107075a1aab331edbbb9b5c5c75e2713b"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/voice/schema.rs
    id: source.e62e0c49477d
    hash: "blake3:bca1f94a9e4f7d668a078bd8d68001caf21c4065fdb2d7f5cb299b29de7e8025"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/voice/secrets.rs
    id: source.4f543bab8457
    hash: "blake3:e892a85b50bdc3dfe39a47a7cfb597681a2bb7d2afd30de282e24293bd69fac4"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/voice/server.rs
    id: source.7196ae6604e2
    hash: "blake3:940d8dbbb16d26db21abecb2b446c537db9f404305b95682d54b9cd8f9f3098a"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: src/voice/webhook.rs
    id: source.5e0c21c80904
    hash: "blake3:4baaabab24c3941037010758eb64f5bf0fe62e42f0771e7182bf982ec947449d"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
claims:
  - id: claim.38da4eaf2310
    claim_kind: extracted
    confidence: high
    cite: "README.md:3"
    source: source.0a096ba47097
    extract: extract.38da4eaf2310
  - id: claim.4322fa4006da
    claim_kind: extracted
    confidence: high
    cite: "README.md:17-19"
    source: source.0a096ba47097
    extract: extract.4322fa4006da
  - id: claim.05968137e921
    claim_kind: extracted
    confidence: high
    cite: "README.md:20"
    source: source.0a096ba47097
    extract: extract.05968137e921
  - id: claim.499eb6fc6f21
    claim_kind: extracted
    confidence: high
    cite: "README.md:21-22"
    source: source.0a096ba47097
    extract: extract.499eb6fc6f21
  - id: claim.d7245d5f8034
    claim_kind: extracted
    confidence: high
    cite: "README.md:30"
    source: source.0a096ba47097
    extract: extract.d7245d5f8034
  - id: claim.ac4883364729
    claim_kind: extracted
    confidence: high
    cite: "README.md:41"
    source: source.0a096ba47097
    extract: extract.ac4883364729
  - id: claim.a31aa9db12d1
    claim_kind: extracted
    confidence: high
    cite: "README.md:58-59"
    source: source.0a096ba47097
    extract: extract.a31aa9db12d1
  - id: claim.3f3a0f2490e4
    claim_kind: extracted
    confidence: high
    cite: "README.md:58-59"
    source: source.0a096ba47097
    extract: extract.3f3a0f2490e4
  - id: claim.c21d234c24da
    claim_kind: extracted
    confidence: high
    cite: "README.md:63"
    source: source.0a096ba47097
    extract: extract.c21d234c24da
  - id: claim.cfa79873ab47
    claim_kind: extracted
    confidence: high
    cite: "README.md:64-65"
    source: source.0a096ba47097
    extract: extract.cfa79873ab47
  - id: claim.4e6596f314b7
    claim_kind: extracted
    confidence: high
    cite: "README.md:66"
    source: source.0a096ba47097
    extract: extract.4e6596f314b7
  - id: claim.e5bab646b9c4
    claim_kind: extracted
    confidence: high
    cite: "README.md:83-86"
    source: source.0a096ba47097
    extract: extract.e5bab646b9c4
  - id: claim.75bbffc50cec
    claim_kind: extracted
    confidence: high
    cite: "README.md:83-86"
    source: source.0a096ba47097
    extract: extract.a197764b1b5a
  - id: claim.e04a6d3a1caf
    claim_kind: extracted
    confidence: high
    cite: "README.md:90-92"
    source: source.0a096ba47097
    extract: extract.e04a6d3a1caf
  - id: claim.8cf2cd9f79f0
    claim_kind: extracted
    confidence: high
    cite: "README.md:90-92"
    source: source.0a096ba47097
    extract: extract.8cf2cd9f79f0
  - id: claim.9a2eb391b49b
    claim_kind: extracted
    confidence: high
    cite: "src/agent/coordinator.rs:268-323"
    source: source.7384604da0f5
    extract: extract.8191c4db2e11
  - id: claim.0baa3c97d403
    claim_kind: extracted
    confidence: high
    cite: "src/agent/coordinator.rs:325-375"
    source: source.7384604da0f5
    extract: extract.15e7132f7674
  - id: claim.2571e8d2d31a
    claim_kind: extracted
    confidence: high
    cite: "src/agent/coordinator.rs:377-394"
    source: source.7384604da0f5
    extract: extract.8e1535b794c1
  - id: claim.96b5a90158b3
    claim_kind: extracted
    confidence: high
    cite: "src/agent/harness.rs:66-76"
    source: source.cf3d53ae512f
    extract: extract.230f1e6da651
  - id: claim.a9111a290609
    claim_kind: extracted
    confidence: high
    cite: "src/agent/harness.rs:78-90"
    source: source.cf3d53ae512f
    extract: extract.3ff1290bdc01
  - id: claim.91042b97a050
    claim_kind: extracted
    confidence: high
    cite: "src/agent/harness.rs:92-95"
    source: source.cf3d53ae512f
    extract: extract.7fb141592d3c
  - id: claim.11d31b8ad0ad
    claim_kind: extracted
    confidence: high
    cite: "src/agent/mod.rs:84-100"
    source: source.0ab2bc446ca3
    extract: extract.82ea3e2b7455
  - id: claim.6cae044e08f2
    claim_kind: extracted
    confidence: high
    cite: "src/agent/mod.rs:102-117"
    source: source.0ab2bc446ca3
    extract: extract.f90ebe75c63f
  - id: claim.532456b95b1d
    claim_kind: extracted
    confidence: high
    cite: "src/agent/mod.rs:159-175"
    source: source.0ab2bc446ca3
    extract: extract.bbc5110623df
  - id: claim.a292f4f4cb0c
    claim_kind: extracted
    confidence: high
    cite: "src/agent/prompts.rs:116-128"
    source: source.8ba8e818b809
    extract: extract.cee460d61e0d
  - id: claim.60f7feb8a378
    claim_kind: extracted
    confidence: high
    cite: "src/agent/prompts.rs:130-136"
    source: source.8ba8e818b809
    extract: extract.e8abd426c081
  - id: claim.0369a41a1c92
    claim_kind: extracted
    confidence: high
    cite: "src/agent/prompts.rs:138-151"
    source: source.8ba8e818b809
    extract: extract.896aed41f76e
extracts:
  - id: extract.38da4eaf2310
    text: "**Feed the impulse to build.**"
    text_hash: "sha256:e71dc636b951a1c206f6bf77df0d76fb24d9f21bb8daaac387c1aee4f0dd53de"
  - id: extract.4322fa4006da
    text: "- **Dioxus cockpit:** `impulse-desktop` is the live, feature-gated desktop path. Dioxus and xterm.js render the system; typed Rust contracts, daemon snapshots, runtime state, and scoped persistence remain authoritative."
    text_hash: "sha256:1eefd4f19bb13c388fcb008393811e3f66a61d650107194026dbb0b6ecc33b11"
  - id: extract.05968137e921
    text: "- **ratatui workbench:** the root crate provides the terminal-native TUI."
    text_hash: "sha256:da1e7915325f6ae8e6dbab3186863e50f07de459cbe109671b8c26f950320a68"
  - id: extract.499eb6fc6f21
    text: "- **CLI and hooks:** short-lived commands initialize projects, track sessions, validate hooks, and query or update daemon-owned state."
    text_hash: "sha256:5af5ad60651fb8221a2699e8181b038bb3aadfe277771f5deaf8c8217c91da30"
  - id: extract.d7245d5f8034
    text: "From this directory:"
    text_hash: "sha256:8c71ab9bcb87c4fee97ea01c904d8ebca66803fef9658a507bec9a1413332bd4"
  - id: extract.ac4883364729
    text: "Launch the feature-gated Dioxus cockpit with:"
    text_hash: "sha256:8adbfc4f720ac9aaa0d2474ff4ce136a83adc831c07583f47aae9a53c704f325"
  - id: extract.a31aa9db12d1
    text: "`impulse-gui` is the legacy/frozen egui workbench."
    text_hash: "sha256:577d78b05f0e09ca3feeb0717b826bd69203f322978a8cf72db6c5091455cc39"
  - id: extract.3f3a0f2490e4
    text: It is excluded from the active Cargo workspace and retained only for compile maintenance while Dioxus owns the current desktop product path.
    text_hash: "sha256:893554a9797e05b0463dc5dd809b00967a2f0da9ef4a737b38d55d0ae7ec6772"
  - id: extract.c21d234c24da
    text: "- The daemon owns live control-plane and workbench snapshots while it is running."
    text_hash: "sha256:c86eef760dcb9a50e81eddbd90cb0ba7c3351b1b4528b049319da0bc92201975"
  - id: extract.cfa79873ab47
    text: "- Project-scoped persistence owns durable history, decisions, configuration, and artifacts across process restarts."
    text_hash: "sha256:baa49ac41a90080a3d8e6bb012f3f667fbb667a3a4eabce7cddd627b047a789a"
  - id: extract.4e6596f314b7
    text: "- PTY runtimes own process and terminal mechanics, then publish structured facts."
    text_hash: "sha256:79a74f0c149cad0761bb85bec08099bf78d44b402a16b5dd4f7a161309e18873"
  - id: extract.e5bab646b9c4
    text: Exact verified test totals change as branches are integrated.
    text_hash: "sha256:396accfa5024b7106bb48c79b2381022e397940e21d446aaafea4df641edb62b"
  - id: extract.a197764b1b5a
    text: "Use the repository-level [`AGENTS.md`](../AGENTS.md) and [`RUST-CANONICAL-CONTRACT.md`](../docs/spec/RUST-CANONICAL-CONTRACT.md) for the current canonical evidence rather than copying counts into this child README."
    text_hash: "sha256:0d2c61b33b305d5859e9defed0ff48ac4ce2df66d750331ec4e9e53159149570"
  - id: extract.e04a6d3a1caf
    text: "Project-local `.impulse/` state includes session history, durable decisions, live session state, and configuration."
    text_hash: "sha256:67adc5aa21800df329289e016e8279f5ba5e153968a8a4a591e5619d8cb1ca93"
  - id: extract.8cf2cd9f79f0
    text: SQLite indexes and daemon/runtime state complement those human-readable artifacts; not every operational record is intended to be committed.
    text_hash: "sha256:ac34fea17d09de8dc4a7b40e5df7ca1518bfea028275ab9a35f02ed7fdc07aa5"
  - id: extract.8191c4db2e11
    text: Detects file conflicts across panes (same file modified by multiple agents).
    text_hash: "sha256:390efac5f13be51d28baa4ab9f2841d168cdcbca98b3ecf7e7fcba31cd904139"
  - id: extract.15e7132f7674
    text: "Detects errors in one pane that might be caused by another pane's changes."
    text_hash: "sha256:0ec15288d9244818daa7c9f259398bd95fd28a6c344435e55141b494606da49a"
  - id: extract.8e1535b794c1
    text: Aggregate all insights into pane-keyed summaries for coordination prompts.
    text_hash: "sha256:a590def135a7c6926e761d4cb154dc0f563cc9965a5e1a4d46164988ba24e6d7"
  - id: extract.230f1e6da651
    text: Create a plain-text fallback response from raw stdout.
    text_hash: "sha256:244085932e258f24f45dfd9528f034acbb4bb614381f3634ba071d9cd378485e"
  - id: extract.3ff1290bdc01
    text: "Attempt to parse stdout as structured JSON, falling back to plain text."
    text_hash: "sha256:4c61ca98fd14e5fdf2e4cebb0d1fbee81394fdb4631515ccffcc59db41197bc8"
  - id: extract.7fb141592d3c
    text: Whether this response was parsed from structured JSON (has model or usage).
    text_hash: "sha256:3762ca160928c72e2111dec1412fe3b225f7014cfb23d38b95989013ae1eed6d"
  - id: extract.82ea3e2b7455
    text: Default model for this provider.
    text_hash: "sha256:5a589ddd969365b0cbb194115f2f78521132af8135223f1cc5c824caa75fca2d"
  - id: extract.f90ebe75c63f
    text: Resolve an API key from config or environment variables.
    text_hash: "sha256:938f7c9e18419e03de6c01b004349bf003c4933015509ad03679b73f091d2373"
  - id: extract.bbc5110623df
    text: "Leading CLI args that put the harness into non-interactive (single-prompt, print-to-stdout) mode. The combined prompt is appended as the final positional argument by the caller."
    text_hash: "sha256:db01b9526c975d76067ae43d830bf39bc4dc569cf1f342195bc8b399eb80bb87"
  - id: extract.cee460d61e0d
    text: Build a user message for code review given pane insights.
    text_hash: "sha256:4693fad36f87b3994a9ca5621c85d55eb74702c809648c10921d0b3d5a0080d5"
  - id: extract.e8abd426c081
    text: Build a user message for error analysis.
    text_hash: "sha256:6ecfda425f2ea8d1a28fb8f9615bff6655ef723149fd304c25d296e4a9254b00"
  - id: extract.896aed41f76e
    text: Build a user message for cross-pane coordination.
    text_hash: "sha256:209ba5b9a1cc8252bc740dfbc23a15e77ffe6d365bc91a2b3af1116033aaaa88"
---

# impulse-rs

**Feed the impulse to build.** (README.md:3)

## Operator surfaces

- **Dioxus cockpit:** `impulse-desktop` is the live, feature-gated desktop path. Dioxus and xterm.js render the system; typed Rust contracts, daemon snapshots, runtime state, and scoped persistence remain authoritative. (README.md:17-19)
- **ratatui workbench:** the root crate provides the terminal-native TUI. (README.md:20)
- **CLI and hooks:** short-lived commands initialize projects, track sessions, validate hooks, and query or update daemon-owned state. (README.md:21-22)

## Run

From this directory: (README.md:30)

```bash
cargo run -- --help
cargo run -- init
cargo run -- daemon
cargo run -- run
cargo run -- session-start -n myproject -p claude-code
cargo run -- validate-hooks --platform claude-code
```

Source: (README.md:32-39)

Launch the feature-gated Dioxus cockpit with: (README.md:41)

```bash
cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop
```

Source: (README.md:43-45)

## Workspace packages

| Package | Responsibility |
| --- | --- |
| `impulse-rs` | CLI, daemon, ratatui workbench, shared services, and native Ion execution path |
| `impulse-desktop` | Dioxus cockpit, xterm.js integration, typed host bridge, and desktop runtime adapters |
| `impulse-ion` | Transport-agnostic Ion harness request/response and adapter contracts |
| `impulse-ops` | Shared control-plane protocol, workbench, policy, registry, artifact, and telemetry models |
| `impulse-step-model` | Deterministic per-step model-routing policy shared across governed runtimes |
| `impulse-term` | Framework-neutral PTY lifecycle, terminal parsing, write queue, and context bridge |

Source: (README.md:49-56)

`impulse-gui` is the legacy/frozen egui workbench. (README.md:58-59)
It is excluded from the active Cargo workspace and retained only for compile maintenance while Dioxus owns the current desktop product path. (README.md:58-59)

## Authority boundaries

- The daemon owns live control-plane and workbench snapshots while it is running. (README.md:63)
- Project-scoped persistence owns durable history, decisions, configuration, and artifacts across process restarts. (README.md:64-65)
- PTY runtimes own process and terminal mechanics, then publish structured facts. (README.md:66)

## Build and verify

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Source: (README.md:76-81)

Exact verified test totals change as branches are integrated. (README.md:83-86)
Use the repository-level `AGENTS.md` and `RUST-CANONICAL-CONTRACT.md` for the current canonical evidence rather than copying counts into this child README. (README.md:83-86)

## Durable project data

Project-local `.impulse/` state includes session history, durable decisions, live session state, and configuration. (README.md:90-92)
SQLite indexes and daemon/runtime state complement those human-readable artifacts; not every operational record is intended to be committed. (README.md:90-92)

## agent

`detect_file_conflicts` — Detects file conflicts across panes (same file modified by multiple agents). (src/agent/coordinator.rs:268-323)
`detect_cross_pane_errors` — Detects errors in one pane that might be caused by another pane's changes. (src/agent/coordinator.rs:325-375)
`aggregate_pane_summaries` — Aggregate all insights into pane-keyed summaries for coordination prompts. (src/agent/coordinator.rs:377-394)
`plain` — Create a plain-text fallback response from raw stdout. (src/agent/harness.rs:66-76)
`parse_or_plain` — Attempt to parse stdout as structured JSON, falling back to plain text. (src/agent/harness.rs:78-90)
`is_structured` — Whether this response was parsed from structured JSON (has model or usage). (src/agent/harness.rs:92-95)
`default_model` — Default model for this provider. (src/agent/mod.rs:84-100)
`resolve_api_key` — Resolve an API key from config or environment variables. (src/agent/mod.rs:102-117)
`invocation_args` — Leading CLI args that put the harness into non-interactive (single-prompt, print-to-stdout) mode. The combined prompt is appended as the final positional argument by the caller. (src/agent/mod.rs:159-175)
`build_review_prompt` — Build a user message for code review given pane insights. (src/agent/prompts.rs:116-128)
`build_error_prompt` — Build a user message for error analysis. (src/agent/prompts.rs:130-136)
`build_coordination_prompt` — Build a user message for cross-pane coordination. (src/agent/prompts.rs:138-151)

## Sources

- [Cargo.toml](../../Cargo.toml)
- [README.md](../../README.md)
- [src/agent/coordinator.rs](../../src/agent/coordinator.rs)
- [src/agent/harness.rs](../../src/agent/harness.rs)
- [src/agent/mod.rs](../../src/agent/mod.rs)
- [src/agent/prompts.rs](../../src/agent/prompts.rs)
- [src/agent/step_model.rs](../../src/agent/step_model.rs)
- [src/agent_discovery/mod.rs](../../src/agent_discovery/mod.rs)
- [src/basis.rs](../../src/basis.rs)
- [src/bin/ion.rs](../../src/bin/ion.rs)
- [src/branding.rs](../../src/branding.rs)
- [src/build_hygiene/clean_all.rs](../../src/build_hygiene/clean_all.rs)
- [src/build_hygiene/discovery.rs](../../src/build_hygiene/discovery.rs)
- [src/build_hygiene/measurement.rs](../../src/build_hygiene/measurement.rs)
- [src/build_hygiene/mod.rs](../../src/build_hygiene/mod.rs)
- [src/build_hygiene/native.rs](../../src/build_hygiene/native.rs)
- [src/build_hygiene/sccache.rs](../../src/build_hygiene/sccache.rs)
- [src/build_hygiene/sweep.rs](../../src/build_hygiene/sweep.rs)
- [src/build_hygiene/tests.rs](../../src/build_hygiene/tests.rs)
- [src/build_hygiene/wipe.rs](../../src/build_hygiene/wipe.rs)
- [src/cli.rs](../../src/cli.rs)
- [src/client/mod.rs](../../src/client/mod.rs)
- [src/context_lifecycle/detector.rs](../../src/context_lifecycle/detector.rs)
- [src/context_lifecycle/extractor.rs](../../src/context_lifecycle/extractor.rs)
- [src/context_lifecycle/injector.rs](../../src/context_lifecycle/injector.rs)
- [src/context_lifecycle/intent.rs](../../src/context_lifecycle/intent.rs)
- [src/context_lifecycle/mod.rs](../../src/context_lifecycle/mod.rs)
- [src/context_lifecycle/monitor.rs](../../src/context_lifecycle/monitor.rs)
- [src/context_lifecycle/parser.rs](../../src/context_lifecycle/parser.rs)
- [src/context_lifecycle/templates.rs](../../src/context_lifecycle/templates.rs)
- [src/context_lifecycle/types.rs](../../src/context_lifecycle/types.rs)
- [src/credentials/cli_proxy.rs](../../src/credentials/cli_proxy.rs)
- [src/credentials/keychain.rs](../../src/credentials/keychain.rs)
- [src/credentials/mod.rs](../../src/credentials/mod.rs)
- [src/credentials/socket.rs](../../src/credentials/socket.rs)
- [src/daemon/handlers.rs](../../src/daemon/handlers.rs)
- [src/daemon/mod.rs](../../src/daemon/mod.rs)
- [src/daemon/protocol.rs](../../src/daemon/protocol.rs)
- [src/daemon/tests.rs](../../src/daemon/tests.rs)
- [src/delegation/detector.rs](../../src/delegation/detector.rs)
- [src/delegation/mod.rs](../../src/delegation/mod.rs)
- [src/delegation/tracker.rs](../../src/delegation/tracker.rs)
- [src/delegation/types.rs](../../src/delegation/types.rs)
- [src/docs/cache.rs](../../src/docs/cache.rs)
- [src/docs/fetch.rs](../../src/docs/fetch.rs)
- [src/docs/mod.rs](../../src/docs/mod.rs)
- [src/docs/models.rs](../../src/docs/models.rs)
- [src/envelope.rs](../../src/envelope.rs)
- [src/error.rs](../../src/error.rs)
- [src/governed_producers.rs](../../src/governed_producers.rs)
- [src/guardrail/config.rs](../../src/guardrail/config.rs)
- [src/guardrail/defaults.rs](../../src/guardrail/defaults.rs)
- [src/guardrail/engine.rs](../../src/guardrail/engine.rs)
- [src/guardrail/mod.rs](../../src/guardrail/mod.rs)
- [src/guardrail/types.rs](../../src/guardrail/types.rs)
- [src/handlers/agent.rs](../../src/handlers/agent.rs)
- [src/handlers/build.rs](../../src/handlers/build.rs)
- [src/handlers/common.rs](../../src/handlers/common.rs)
- [src/handlers/config.rs](../../src/handlers/config.rs)
- [src/handlers/daemon_dispatch.rs](../../src/handlers/daemon_dispatch.rs)
- [src/handlers/describe.rs](../../src/handlers/describe.rs)
- [src/handlers/direct_dispatch.rs](../../src/handlers/direct_dispatch.rs)
- [src/handlers/guard.rs](../../src/handlers/guard.rs)
- [src/handlers/injection_handlers.rs](../../src/handlers/injection_handlers.rs)
- [src/handlers/ion.rs](../../src/handlers/ion.rs)
- [src/handlers/memory.rs](../../src/handlers/memory.rs)
- [src/handlers/mod.rs](../../src/handlers/mod.rs)
- [src/handlers/office.rs](../../src/handlers/office.rs)
- [src/handlers/plugin_handlers.rs](../../src/handlers/plugin_handlers.rs)
- [src/handlers/retrieval.rs](../../src/handlers/retrieval.rs)
- [src/handlers/semantic_diff_handlers.rs](../../src/handlers/semantic_diff_handlers.rs)
- [src/handlers/session.rs](../../src/handlers/session.rs)
- [src/handlers/stewardship_handlers.rs](../../src/handlers/stewardship_handlers.rs)
- [src/handlers/system.rs](../../src/handlers/system.rs)
- [src/handlers/tooling_handlers.rs](../../src/handlers/tooling_handlers.rs)
- [src/handlers/voice_handlers.rs](../../src/handlers/voice_handlers.rs)
- [src/injection/engine.rs](../../src/injection/engine.rs)
- [src/injection/mod.rs](../../src/injection/mod.rs)
- [src/injection/staging.rs](../../src/injection/staging.rs)
- [src/injection/types.rs](../../src/injection/types.rs)
- [src/integration_tests.rs](../../src/integration_tests.rs)
- [src/ion_repl/chat.rs](../../src/ion_repl/chat.rs)
- [src/ion_repl/history.rs](../../src/ion_repl/history.rs)
- [src/ion_repl/mod.rs](../../src/ion_repl/mod.rs)
- [src/ion_repl/registry.rs](../../src/ion_repl/registry.rs)
- [src/ion_repl/router.rs](../../src/ion_repl/router.rs)
- [src/ion_repl/tool_bridge.rs](../../src/ion_repl/tool_bridge.rs)
- [src/ion_repl/tool_claim.rs](../../src/ion_repl/tool_claim.rs)
- [src/ion_repl/tool_verify.rs](../../src/ion_repl/tool_verify.rs)
- [src/ion_repl/tools.rs](../../src/ion_repl/tools.rs)
- [src/lib.rs](../../src/lib.rs)
- [src/llm_backends/anthropic.rs](../../src/llm_backends/anthropic.rs)
- [src/llm_backends/mod.rs](../../src/llm_backends/mod.rs)
- [src/main.rs](../../src/main.rs)
- [src/mcp/mod.rs](../../src/mcp/mod.rs)
- [src/mcp/server.rs](../../src/mcp/server.rs)
- [src/memory/mod.rs](../../src/memory/mod.rs)
- [src/monty/datafusion.rs](../../src/monty/datafusion.rs)
- [src/monty/kdb.rs](../../src/monty/kdb.rs)
- [src/monty/mod.rs](../../src/monty/mod.rs)
- [src/monty/python.rs](../../src/monty/python.rs)
- [src/monty/routing.rs](../../src/monty/routing.rs)
- [src/monty/swarm.rs](../../src/monty/swarm.rs)
- [src/notification/mod.rs](../../src/notification/mod.rs)
- [src/office/excel.rs](../../src/office/excel.rs)
- [src/office/extraction.rs](../../src/office/extraction.rs)
- [src/office/mod.rs](../../src/office/mod.rs)
- [src/office/word.rs](../../src/office/word.rs)
- [src/ops_workbench.rs](../../src/ops_workbench.rs)
- [src/orchestration/mod.rs](../../src/orchestration/mod.rs)
- [src/plugin/action.rs](../../src/plugin/action.rs)
- [src/plugin/context.rs](../../src/plugin/context.rs)
- [src/plugin/mod.rs](../../src/plugin/mod.rs)
- [src/plugin/registry.rs](../../src/plugin/registry.rs)
- [src/process_group.rs](../../src/process_group.rs)
- [src/process_util.rs](../../src/process_util.rs)
- [src/retrieval/embedding.rs](../../src/retrieval/embedding.rs)
- [src/retrieval/fuzzy.rs](../../src/retrieval/fuzzy.rs)
- [src/retrieval/indexer.rs](../../src/retrieval/indexer.rs)
- [src/retrieval/mod.rs](../../src/retrieval/mod.rs)
- [src/retrieval/pageindex.rs](../../src/retrieval/pageindex.rs)
- [src/retrieval/query.rs](../../src/retrieval/query.rs)
- [src/retrieval/store.rs](../../src/retrieval/store.rs)
- [src/retrieval/types.rs](../../src/retrieval/types.rs)
- [src/semantic_diff/mod.rs](../../src/semantic_diff/mod.rs)
- [src/semantic_diff/runner.rs](../../src/semantic_diff/runner.rs)
- [src/semantic_diff/types.rs](../../src/semantic_diff/types.rs)
- [src/settlement.rs](../../src/settlement.rs)
- [src/state/config.rs](../../src/state/config.rs)
- [src/state/config_keys/mod.rs](../../src/state/config_keys/mod.rs)
- [src/state/config_keys/rules.rs](../../src/state/config_keys/rules.rs)
- [src/state/config_keys/tests.rs](../../src/state/config_keys/tests.rs)
- [src/state/governed_task.rs](../../src/state/governed_task.rs)
- [src/state/memory_candidate.rs](../../src/state/memory_candidate.rs)
- [src/state/mod.rs](../../src/state/mod.rs)
- [src/state/persistence.rs](../../src/state/persistence.rs)
- [src/state/session.rs](../../src/state/session.rs)
- [src/stewardship/analyzer.rs](../../src/stewardship/analyzer.rs)
- [src/stewardship/approval.rs](../../src/stewardship/approval.rs)
- [src/stewardship/cleanup.rs](../../src/stewardship/cleanup.rs)
- [src/stewardship/cross_project.rs](../../src/stewardship/cross_project.rs)
- [src/stewardship/mod.rs](../../src/stewardship/mod.rs)
- [src/stewardship/monitor.rs](../../src/stewardship/monitor.rs)
- [src/stewardship/types.rs](../../src/stewardship/types.rs)
- [src/storage/mod.rs](../../src/storage/mod.rs)
- [src/test_support.rs](../../src/test_support.rs)
- [src/token_tracker/algorithm.rs](../../src/token_tracker/algorithm.rs)
- [src/token_tracker/cross_platform.rs](../../src/token_tracker/cross_platform.rs)
- [src/token_tracker/metrics.rs](../../src/token_tracker/metrics.rs)
- [src/token_tracker/mod.rs](../../src/token_tracker/mod.rs)
- [src/token_tracker/research.rs](../../src/token_tracker/research.rs)
- [src/token_tracker/types.rs](../../src/token_tracker/types.rs)
- [src/tooling/builtin/bash_exec.rs](../../src/tooling/builtin/bash_exec.rs)
- [src/tooling/builtin/benchmarker.rs](../../src/tooling/builtin/benchmarker.rs)
- [src/tooling/builtin/build_health.rs](../../src/tooling/builtin/build_health.rs)
- [src/tooling/builtin/calculator.rs](../../src/tooling/builtin/calculator.rs)
- [src/tooling/builtin/config_get.rs](../../src/tooling/builtin/config_get.rs)
- [src/tooling/builtin/document_extract.rs](../../src/tooling/builtin/document_extract.rs)
- [src/tooling/builtin/file_read.rs](../../src/tooling/builtin/file_read.rs)
- [src/tooling/builtin/file_write.rs](../../src/tooling/builtin/file_write.rs)
- [src/tooling/builtin/genome_read.rs](../../src/tooling/builtin/genome_read.rs)
- [src/tooling/builtin/health_check.rs](../../src/tooling/builtin/health_check.rs)
- [src/tooling/builtin/memory_search.rs](../../src/tooling/builtin/memory_search.rs)
- [src/tooling/builtin/mod.rs](../../src/tooling/builtin/mod.rs)
- [src/tooling/builtin/python_exec.rs](../../src/tooling/builtin/python_exec.rs)
- [src/tooling/builtin/session_query.rs](../../src/tooling/builtin/session_query.rs)
- [src/tooling/builtin/steward_status.rs](../../src/tooling/builtin/steward_status.rs)
- [src/tooling/builtin/system_info.rs](../../src/tooling/builtin/system_info.rs)
- [src/tooling/document/document_parse.rs](../../src/tooling/document/document_parse.rs)
- [src/tooling/document/excel_read.rs](../../src/tooling/document/excel_read.rs)
- [src/tooling/document/mod.rs](../../src/tooling/document/mod.rs)
- [src/tooling/document/word_read.rs](../../src/tooling/document/word_read.rs)
- [src/tooling/env_scrub.rs](../../src/tooling/env_scrub.rs)
- [src/tooling/error.rs](../../src/tooling/error.rs)
- [src/tooling/executor.rs](../../src/tooling/executor.rs)
- [src/tooling/external.rs](../../src/tooling/external.rs)
- [src/tooling/mod.rs](../../src/tooling/mod.rs)
- [src/tooling/registry.rs](../../src/tooling/registry.rs)
- [src/tooling/traits.rs](../../src/tooling/traits.rs)
- [src/tools/benchmark.rs](../../src/tools/benchmark.rs)
- [src/tools/health.rs](../../src/tools/health.rs)
- [src/tools/init.rs](../../src/tools/init.rs)
- [src/tools/list.rs](../../src/tools/list.rs)
- [src/tools/mod.rs](../../src/tools/mod.rs)
- [src/tools/python.rs](../../src/tools/python.rs)
- [src/tools/system.rs](../../src/tools/system.rs)
- [src/tools/update.rs](../../src/tools/update.rs)
- [src/ui/agent_terminal.rs](../../src/ui/agent_terminal.rs)
- [src/ui/lifecycle.rs](../../src/ui/lifecycle.rs)
- [src/ui/mod.rs](../../src/ui/mod.rs)
- [src/ui/pane_manager.rs](../../src/ui/pane_manager.rs)
- [src/ui/render_content.rs](../../src/ui/render_content.rs)
- [src/ui/render_dashboard.rs](../../src/ui/render_dashboard.rs)
- [src/ui/render_menu.rs](../../src/ui/render_menu.rs)
- [src/ui/render_status.rs](../../src/ui/render_status.rs)
- [src/ui/render_tabs.rs](../../src/ui/render_tabs.rs)
- [src/ui/runner.rs](../../src/ui/runner.rs)
- [src/ui/terminal_pane.rs](../../src/ui/terminal_pane.rs)
- [src/ui/types.rs](../../src/ui/types.rs)
- [src/ui/visualization.rs](../../src/ui/visualization.rs)
- [src/validate.rs](../../src/validate.rs)
- [src/verify/mod.rs](../../src/verify/mod.rs)
- [src/voice/adapter.rs](../../src/voice/adapter.rs)
- [src/voice/envelope.rs](../../src/voice/envelope.rs)
- [src/voice/mod.rs](../../src/voice/mod.rs)
- [src/voice/policy.rs](../../src/voice/policy.rs)
- [src/voice/provider.rs](../../src/voice/provider.rs)
- [src/voice/schema.rs](../../src/voice/schema.rs)
- [src/voice/secrets.rs](../../src/voice/secrets.rs)
- [src/voice/server.rs](../../src/voice/server.rs)
- [src/voice/webhook.rs](../../src/voice/webhook.rs)

## Symbols

- `function` `detect_file_conflicts`
- `function` `detect_cross_pane_errors`
- `function` `aggregate_pane_summaries`
- `function` `plain`
- `function` `parse_or_plain`
- `function` `is_structured`
- `function` `default_model`
- `function` `resolve_api_key`
- `function` `invocation_args`
- `function` `build_review_prompt`
- `function` `build_error_prompt`
- `function` `build_coordination_prompt`
