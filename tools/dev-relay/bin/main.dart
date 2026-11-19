/// `dart run tools/dev-relay/bin/main.dart --port 7000` — a development relay (#268).
///
/// It prints the two commands that use it, because the whole point of this program is that a person with a phone can
/// run the checklist without being told what to type.
library;

import 'dart:async';
import 'dart:io';

import 'package:dev_relay/relay.dart';

Future<void> main(List<String> argv) async {
  var host = '127.0.0.1';
  var port = 7000;
  var verbose = false;

  for (var index = 0; index < argv.length; index++) {
    switch (argv[index]) {
      case '--host':
        host = _value(argv, ++index, '--host');
      case '--port':
        port = int.tryParse(_value(argv, ++index, '--port')) ?? _usage('--port takes a number');
      case '--verbose':
        verbose = true;
      case '-h' || '--help':
        _help();
        return;
      default:
        _usage('unknown argument ${argv[index]}');
    }
  }

  final relay = DevRelay(
    report: verbose
        ? (message) {
            stdout.writeln('[relay] $message');
          }
        : null,
  );
  await relay.start(host: host, port: port);
  final at = '${host.contains(':') ? '[$host]' : host}:${relay.port}';
  stdout
    ..writeln('[relay] listening on $at')
    ..writeln('[relay]   wall:  ARCONS_RELAY_URL=tcp://$at cargo run')
    ..writeln('[relay]   phone: flutter run --dart-define=ARCONS_RELAY=tcp://<this machine>:$port')
    ..writeln('[relay] not the contract: no auth, no TLS, no persistence (docs/control.md)');

  // Ctrl-C closes the sockets rather than leaving a wall redialling into a dead port.
  final stop = Completer<void>();
  late final StreamSubscription<ProcessSignal> interrupts;
  interrupts = ProcessSignal.sigint.watch().listen((_) {
    if (!stop.isCompleted) {
      stop.complete();
    }
  });
  await stop.future;
  await interrupts.cancel();
  await relay.close();
}

String _value(List<String> argv, int index, String flag) {
  if (index >= argv.length) {
    _usage('$flag needs a value');
  }
  return argv[index];
}

void _help() {
  stdout
    ..writeln('a development relay for the phone control plane (#268)')
    ..writeln('')
    ..writeln('  --host <addr>   listen address (default 127.0.0.1; 0.0.0.0 for a phone on the LAN)')
    ..writeln('  --port <n>      listen port (default 7000)')
    ..writeln('  --verbose       log frames, with the device key and the pairing key redacted')
    ..writeln('  -h, --help      this');
}

Never _usage(String why) {
  stderr.writeln('dev-relay: $why');
  _help();
  exit(64);
}
