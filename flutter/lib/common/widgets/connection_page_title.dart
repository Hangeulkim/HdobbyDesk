import 'package:auto_size_text/auto_size_text.dart';
import 'package:flutter/material.dart';
import 'package:get/get.dart';

import '../../common.dart';

Widget getConnectionPageTitle(BuildContext context, bool isWeb) {
  return Row(
    children: [
      Expanded(
          child: Row(
        children: [
          AutoSizeText(
            translate('Control Remote Desktop'),
            maxLines: 1,
            style: Theme.of(context)
                .textTheme
                .titleLarge
                ?.merge(TextStyle(height: 1)),
          ).marginOnly(right: 4),
          Tooltip(
            waitDuration: Duration(milliseconds: 300),
            message: hdobbyLanguage(context).startsWith('ko')
                ? '직접 연결 준비에서 상대의 연결 코드를 붙여 넣고 인증서 지문을 확인하세요. 주소를 직접 입력할 수도 있습니다. 내부 ID는 사용자가 설정한 내부 서버가 있을 때만 사용할 수 있습니다.'
                : 'Paste the host connection code in Prepare direct connection and verify its fingerprint. You can also enter an address. Internal IDs require your configured internal server.',
            child: Icon(
              Icons.help_outline_outlined,
              size: 16,
              color: Theme.of(context)
                  .textTheme
                  .titleLarge
                  ?.color
                  ?.withOpacity(0.5),
            ),
          ),
        ],
      )),
    ],
  );
}
