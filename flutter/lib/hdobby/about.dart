import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

String hdobbyAboutTitle({required bool korean, required String appName}) =>
    korean ? '$appName 정보' : 'About $appName';

/// Local notice: opening this view never contacts a website.
Future<void> showHdobbyNotices(BuildContext context, {required bool korean}) {
  final notice = rootBundle.loadString('assets/OPEN_SOURCE_NOTICES.txt');
  return showDialog<void>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(korean ? '오픈소스 고지' : 'Open-source notices'),
      content: SizedBox(
        width: 640,
        height: 480,
        child: FutureBuilder<String>(
          future: notice,
          builder: (context, snapshot) {
            if (snapshot.hasError) {
              return Text(korean
                  ? '고지 파일을 불러오지 못했습니다. 배포 파일을 확인해 주세요.'
                  : 'The notice file could not be loaded. Check this app package.');
            }
            if (!snapshot.hasData) {
              return const Center(child: CircularProgressIndicator());
            }
            return SingleChildScrollView(child: SelectableText(snapshot.data!));
          },
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(korean ? '닫기' : 'Close'),
        ),
      ],
    ),
  );
}
